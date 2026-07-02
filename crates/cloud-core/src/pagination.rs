//! 通用分页:把"逐页拉取"的接口自动展开成一条按元素产出的流。
//!
//! 对象存储的 list 接口都是分页的(截断时返回下一页游标)。各 SDK 只需提供
//! 一个"给定游标 → 拉一页"的异步闭包,这里负责反复翻页直到没有下一页,
//! 并把每页的元素摊平成 [`Stream`]。与厂商无关:游标类型 `C` 由 SDK 自定
//! (OSS 的 marker、COS 的 next-marker、S3 的 continuation-token 皆可)。

use std::future::Future;

use async_stream::try_stream;
use futures::Stream;

/// 一页结果:本页元素 + 下一页游标(`None` 表示已到末页)。
#[derive(Debug, Clone)]
pub struct Page<T, C> {
    pub items: Vec<T>,
    pub next: Option<C>,
}

impl<T, C> Page<T, C> {
    /// 构造最后一页(无下一页游标)。
    pub fn last(items: Vec<T>) -> Self {
        Self { items, next: None }
    }
}

/// 从 `initial` 游标开始反复调用 `fetch`,产出一条 `Result<T, E>` 的流。
///
/// 任一页 `fetch` 出错时,流产出该 `Err` 后结束。
pub fn paginate<T, E, C, F, Fut>(
    initial: C,
    mut fetch: F,
) -> impl Stream<Item = Result<T, E>>
where
    F: FnMut(C) -> Fut,
    Fut: Future<Output = Result<Page<T, C>, E>>,
{
    try_stream! {
        let mut cursor = Some(initial);
        while let Some(c) = cursor.take() {
            let page = fetch(c).await?;
            for item in page.items {
                yield item;
            }
            cursor = page.next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::cell::Cell;

    // 用内存里的分页数据模拟一个列表接口:每页 2 个元素,共 5 个。
    #[tokio::test]
    async fn walks_all_pages_in_order() {
        let pages_data: Vec<Vec<i32>> = vec![vec![1, 2], vec![3, 4], vec![5]];
        let pages = &pages_data; // &Vec 是 Copy,可在每次 fetch 的 future 里共享
        let fetches_cell = Cell::new(0);
        let fetches = &fetches_cell;

        // 游标 = 页索引 usize。
        let stream = paginate(0usize, |idx: usize| async move {
            fetches.set(fetches.get() + 1);
            let items = pages[idx].clone();
            let next = if idx + 1 < pages.len() { Some(idx + 1) } else { None };
            Ok::<_, ()>(Page { items, next })
        });
        futures::pin_mut!(stream);

        let collected: Vec<i32> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(collected, vec![1, 2, 3, 4, 5]);
        assert_eq!(fetches.get(), 3); // 恰好翻了 3 页
    }

    // 空结果:首页即末页且无元素。
    #[tokio::test]
    async fn handles_empty_result() {
        let stream = paginate(0usize, |_| async {
            Ok::<_, ()>(Page::<i32, usize>::last(vec![]))
        });
        futures::pin_mut!(stream);
        let collected: Vec<Result<i32, ()>> = stream.collect().await;
        assert!(collected.is_empty());
    }

    // 中途出错:已产出的元素照常,错误作为最后一项产出后终止。
    #[tokio::test]
    async fn stops_and_surfaces_error() {
        let stream = paginate(0usize, |idx: usize| async move {
            if idx == 0 {
                Ok(Page { items: vec![10, 20], next: Some(1) })
            } else {
                Err("boom")
            }
        });
        futures::pin_mut!(stream);

        let mut oks = Vec::new();
        let mut err = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(v) => oks.push(v),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert_eq!(oks, vec![10, 20]);
        assert_eq!(err, Some("boom"));
    }
}
