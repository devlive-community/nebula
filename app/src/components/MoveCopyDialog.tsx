import { useState } from "react";

interface Props {
  /** 源对象完整路径,如 bucket/a/b.txt。 */
  from: string;
  onCopy: (to: string) => void;
  onMove: (to: string) => void;
  onCancel: () => void;
}

/** 复制 / 移动对象到指定目标路径(支持跨目录 / 跨桶)。 */
export function MoveCopyDialog({ from, onCopy, onMove, onCancel }: Props) {
  const [to, setTo] = useState(from);
  const valid = to.trim() !== "" && to.trim() !== from;
  const dest = to.trim();

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>复制 / 移动到</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>源</span>
            <input value={from} disabled />
          </label>
          <label className="field">
            <span>目标路径(bucket/key,可跨桶)</span>
            <input
              value={to}
              autoFocus
              onChange={(e) => setTo(e.target.value)}
              placeholder="如 other-bucket/dir/name.txt"
            />
          </label>
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button className="btn" disabled={!valid} onClick={() => onMove(dest)}>
            移动
          </button>
          <button
            className="btn btn--primary"
            disabled={!valid}
            onClick={() => onCopy(dest)}
          >
            复制
          </button>
        </div>
      </div>
    </div>
  );
}
