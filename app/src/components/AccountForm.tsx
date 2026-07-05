import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";

interface Props {
  /** 编辑模式的回填值;不传为新增。 */
  initial?: { id: string; accessKeyId: string; endpoint: string };
  onSubmit: (
    id: string,
    accessKeyId: string,
    accessKeySecret: string,
    endpoint: string,
  ) => void;
  onClose: () => void;
}

export function AccountForm({ initial, onSubmit, onClose }: Props) {
  const editing = !!initial;
  const [id, setId] = useState(initial?.id ?? "");
  const [ak, setAk] = useState(initial?.accessKeyId ?? "");
  const [sk, setSk] = useState("");
  const [endpoint, setEndpoint] = useState(
    initial?.endpoint ?? "oss-cn-hangzhou.aliyuncs.com",
  );

  const valid = id && ak && sk && endpoint;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{editing ? "编辑阿里云 OSS 账号" : "添加阿里云 OSS 账号"}</h3>
          <button className="modal__close" onClick={onClose}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>

        <div className="modal__body">
          <label className="field">
            <span>账号别名</span>
            <input
              value={id}
              onChange={(e) => setId(e.target.value)}
              placeholder="如 aliyun-main"
              disabled={editing}
              autoFocus={!editing}
            />
          </label>
          <label className="field">
            <span>AccessKeyId</span>
            <input value={ak} onChange={(e) => setAk(e.target.value)} />
          </label>
          <label className="field">
            <span>AccessKeySecret{editing ? "(请重新输入)" : ""}</span>
            <input
              type="password"
              value={sk}
              onChange={(e) => setSk(e.target.value)}
            />
          </label>
          <label className="field">
            <span>Endpoint</span>
            <input
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder="oss-cn-hangzhou.aliyuncs.com"
            />
          </label>
        </div>

        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button
            className="btn btn--primary"
            disabled={!valid}
            onClick={() => onSubmit(id, ak, sk, endpoint)}
          >
            {editing ? "保存" : "添加"}
          </button>
        </div>
      </div>
    </div>
  );
}
