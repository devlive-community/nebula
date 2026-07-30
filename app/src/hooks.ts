import { useEffect, useRef, useState, type UIEvent } from "react";

/**
 * 增量渲染:一次只渲染前 `batch` 项,滚动到接近底部时再追加一批。
 *
 * 对象存储的一个目录 / 桶可能有上万个对象,一次性把这么多行塞进 DOM 会明显卡顿。
 * 这里不改变数据获取(仍是拿到完整列表),只控制**渲染进入 DOM 的数量**——滚到底部
 * 附近才继续放行,行为上就是"下滑分批加载"。
 *
 * 列表引用变化(切目录 / 过滤 / 排序)时自动重置回第一批。
 *
 * 若首批渲染出的内容本身就没填满容器(内容比视口矮,没有可滚动的溢出),浏览器根本
 * 不会派发 scroll 事件——单靠 onScroll 驱动会导致分页永久卡住。用 `containerRef`
 * 拿到容器 DOM,每次批次 / 总数变化后检查一次是否"没有溢出但还有更多",没有就自动放行,
 * 直到内容填满容器或全部展示完。
 */
export function useIncremental<T>(
  items: T[],
  batch = 120,
  onReachEnd?: () => void,
) {
  const [count, setCount] = useState(batch);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setCount(batch);
  }, [items, batch]);

  const hasMore = count < items.length;

  // 内容不足以撑满容器(无可滚动溢出)时,滚动事件永远不会触发——主动放行下一批;
  // 已加载的全展示完仍未填满,则直接请求上层拉下一页(`onReachEnd` 内部会做并发 /
  // 无游标兜底,重复调用安全)。直至溢出出现、或已加载 + 后端都没有更多。
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (el.scrollHeight - el.clientHeight < 320) {
      if (hasMore) setCount((c) => Math.min(c + batch, items.length));
      else onReachEnd?.();
    }
  }, [hasMore, count, items.length, batch, onReachEnd]);

  const onScroll = (e: UIEvent<HTMLElement>) => {
    const el = e.currentTarget;
    // 距底部 320px 内触发。
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 320) {
      if (count < items.length) {
        // 还有已加载但未渲染的,先放行下一批进 DOM。
        setCount((c) => Math.min(c + batch, items.length));
      } else {
        // 已加载的全部渲染完 → 请求上层拉取下一页(若还有)。
        onReachEnd?.();
      }
    }
  };

  return {
    shown: count < items.length ? items.slice(0, count) : items,
    shownCount: Math.min(count, items.length),
    total: items.length,
    hasMore,
    onScroll,
    containerRef,
  };
}
