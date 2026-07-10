import { useEffect, useState, type UIEvent } from "react";

/**
 * 增量渲染:一次只渲染前 `batch` 项,滚动到接近底部时再追加一批。
 *
 * 对象存储的一个目录 / 桶可能有上万个对象,一次性把这么多行塞进 DOM 会明显卡顿。
 * 这里不改变数据获取(仍是拿到完整列表),只控制**渲染进入 DOM 的数量**——滚到底部
 * 附近才继续放行,行为上就是"下滑分批加载"。
 *
 * 列表引用变化(切目录 / 过滤 / 排序)时自动重置回第一批。
 */
export function useIncremental<T>(items: T[], batch = 120) {
  const [count, setCount] = useState(batch);

  useEffect(() => {
    setCount(batch);
  }, [items, batch]);

  const onScroll = (e: UIEvent<HTMLElement>) => {
    const el = e.currentTarget;
    // 距底部 320px 内就预取下一批,避免滚到最底才加载的顿挫。
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 320) {
      setCount((c) => (c < items.length ? Math.min(c + batch, items.length) : c));
    }
  };

  return {
    shown: count < items.length ? items.slice(0, count) : items,
    shownCount: Math.min(count, items.length),
    total: items.length,
    hasMore: count < items.length,
    onScroll,
  };
}
