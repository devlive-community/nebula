import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";
import { Select } from "./Select";
import { VENDORS, type Vendor } from "../vendors";

interface Props {
  /** 编辑模式的回填值;不传为新增。 */
  initial?: { id: string; vendor: string; accessKeyId: string; endpoint: string };
  onSubmit: (
    vendor: Vendor,
    id: string,
    accessKeyId: string,
    accessKeySecret: string,
    endpoint: string,
  ) => void;
  onClose: () => void;
}

export function AccountForm({ initial, onSubmit, onClose }: Props) {
  const editing = !!initial;
  const [vendor, setVendor] = useState<Vendor>(
    (initial?.vendor as Vendor) in VENDORS ? (initial!.vendor as Vendor) : "aliyun",
  );
  const [id, setId] = useState(initial?.id ?? "");
  const [ak, setAk] = useState(initial?.accessKeyId ?? "");
  const [sk, setSk] = useState("");
  // 新增时 endpoint 跟随厂商默认;用户手动改过则不再自动覆盖。
  const [endpoint, setEndpoint] = useState(
    initial?.endpoint ?? VENDORS[vendor].endpoint,
  );
  const [endpointTouched, setEndpointTouched] = useState(editing);

  const meta = VENDORS[vendor];

  // 切换厂商时,若用户未手动改过 endpoint,则套用新厂商的默认 endpoint。
  const changeVendor = (next: Vendor) => {
    setVendor(next);
    if (!endpointTouched) setEndpoint(VENDORS[next].endpoint);
  };

  const valid = id && ak && sk && endpoint;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {editing ? `编辑${meta.label} 账号` : `添加${meta.label} 账号`}
          </h3>
          <button className="modal__close" onClick={onClose}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>

        <div className="modal__body">
          {/* 用 div 而非 label:label 会把点击转发给关联控件,导致选项点击后
              触发器又被 toggle 回打开状态。 */}
          <div className="field">
            <span>云厂商</span>
            <Select
              value={vendor}
              disabled={editing}
              onChange={(v) => changeVendor(v as Vendor)}
              options={Object.entries(VENDORS).map(([key, v]) => ({
                value: key,
                label: v.label,
              }))}
            />
          </div>
          <label className="field">
            <span>账号别名</span>
            <input
              value={id}
              onChange={(e) => setId(e.target.value)}
              placeholder={meta.idPlaceholder}
              disabled={editing}
              autoFocus={!editing}
            />
          </label>
          <label className="field">
            <span>{meta.akLabel}</span>
            <input value={ak} onChange={(e) => setAk(e.target.value)} />
          </label>
          <label className="field">
            <span>
              {meta.skLabel}
              {editing ? "(请重新输入)" : ""}
            </span>
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
              onChange={(e) => {
                setEndpoint(e.target.value);
                setEndpointTouched(true);
              }}
              placeholder={meta.endpoint}
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
            onClick={() => onSubmit(vendor, id, ak, sk, endpoint)}
          >
            {editing ? "保存" : "添加"}
          </button>
        </div>
      </div>
    </div>
  );
}
