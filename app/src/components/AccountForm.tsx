import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";
import { Select } from "./Select";
import { VENDORS, type Vendor } from "../vendors";

interface Props {
  /** 编辑模式的回填值;不传为新增。 */
  initial?: {
    id: string;
    vendor: string;
    accessKeyId: string;
    endpoint: string;
    customDomain?: string;
  };
  onSubmit: (
    vendor: Vendor,
    id: string,
    accessKeyId: string,
    accessKeySecret: string,
    endpoint: string,
    customDomain: string,
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
  const [domain, setDomain] = useState(initial?.customDomain ?? "");

  const meta = VENDORS[vendor];

  // 切换厂商时,若用户未手动改过 endpoint,则套用新厂商的默认 endpoint。
  const changeVendor = (next: Vendor) => {
    setVendor(next);
    if (!endpointTouched) setEndpoint(VENDORS[next].endpoint);
  };

  // 纯改别名(其它字段都没变)不强制重新输入密钥——密钥只在真的要更新凭证时才必填,
  // 不然用户光是想改个名字都要翻出密钥重新输一遍。
  const onlyIdChanged =
    editing &&
    id !== initial!.id &&
    ak === initial!.accessKeyId &&
    endpoint === initial!.endpoint &&
    domain === (initial!.customDomain ?? "");
  const valid = editing
    ? !!id && !!ak && !!endpoint && (!!sk || onlyIdChanged)
    : !!id && !!ak && !!sk && !!endpoint;

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
              {editing ? (onlyIdChanged ? "(仅改别名可留空)" : "(请重新输入)") : ""}
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
          <label className="field">
            <span>公共域名(可选)</span>
            <input
              value={domain}
              onChange={(e) => setDomain(e.target.value)}
              placeholder="cdn.example.com — 设为公开读后用它拼永久直链"
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
            onClick={() => onSubmit(vendor, id, ak, sk, endpoint, domain)}
          >
            {editing ? "保存" : "添加"}
          </button>
        </div>
      </div>
    </div>
  );
}
