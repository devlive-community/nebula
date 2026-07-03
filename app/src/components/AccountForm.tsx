import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";

interface Props {
  onSubmit: (
    id: string,
    accessKeyId: string,
    accessKeySecret: string,
    endpoint: string,
  ) => void;
  onClose: () => void;
}

export function AccountForm({ onSubmit, onClose }: Props) {
  const [id, setId] = useState("");
  const [ak, setAk] = useState("");
  const [sk, setSk] = useState("");
  const [endpoint, setEndpoint] = useState("oss-cn-hangzhou.aliyuncs.com");

  const valid = id && ak && sk && endpoint;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>添加阿里云 OSS 账号</h3>
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
              autoFocus
            />
          </label>
          <label className="field">
            <span>AccessKeyId</span>
            <input value={ak} onChange={(e) => setAk(e.target.value)} />
          </label>
          <label className="field">
            <span>AccessKeySecret</span>
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
            添加
          </button>
        </div>
      </div>
    </div>
  );
}
