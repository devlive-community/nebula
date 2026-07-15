import { useState } from "react";
import { Select } from "./Select";
import { useI18n } from "../i18n";

/** 各厂商可选的存储类型(value 为发给后端的原始字符串)。新增厂商在此追加。 */
const STORAGE_CLASSES: Record<string, { value: string; label: string }[]> = {
  aliyun: [
    { value: "Standard", label: "标准" },
    { value: "IA", label: "低频访问" },
    { value: "Archive", label: "归档" },
    { value: "ColdArchive", label: "冷归档" },
  ],
  huawei: [
    { value: "STANDARD", label: "标准" },
    { value: "WARM", label: "低频 (WARM)" },
    { value: "COLD", label: "归档 (COLD)" },
  ],
  tencent: [
    { value: "STANDARD", label: "标准" },
    { value: "STANDARD_IA", label: "低频访问" },
    { value: "ARCHIVE", label: "归档" },
    { value: "DEEP_ARCHIVE", label: "深度归档" },
  ],
  aws: [
    { value: "STANDARD", label: "标准" },
    { value: "STANDARD_IA", label: "低频访问" },
    { value: "INTELLIGENT_TIERING", label: "智能分层" },
    { value: "GLACIER", label: "归档 (Glacier)" },
    { value: "DEEP_ARCHIVE", label: "深度归档" },
  ],
  r2: [
    { value: "Standard", label: "标准" },
    { value: "InfrequentAccess", label: "低频访问" },
  ],
  minio: [
    { value: "STANDARD", label: "标准" },
    { value: "REDUCED_REDUNDANCY", label: "低冗余" },
  ],
  qiniu: [
    { value: "STANDARD", label: "标准" },
    { value: "LINE", label: "低频 (LINE)" },
    { value: "ARCHIVE", label: "归档" },
    { value: "DEEP_ARCHIVE", label: "深度归档" },
  ],
};

interface ClassProps {
  vendor: string;
  name: string;
  current: string | null;
  onConfirm: (storageClass: string) => void;
  onCancel: () => void;
}

/** 转换存储类型:按账号所属厂商列出可选层级。 */
export function StorageClassDialog({ vendor, name, current, onConfirm, onCancel }: ClassProps) {
  const { t } = useI18n();
  const options = STORAGE_CLASSES[vendor] ?? [{ value: "STANDARD", label: "标准" }];
  const [sel, setSel] = useState(
    options.find((o) => o.value === current)?.value ?? options[0].value,
  );

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("转换存储类型")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("对象:{name}", { name })}</span>
          </label>
          <label className="field">
            <span>{t("目标存储类型")}</span>
            <Select
              value={sel}
              options={options.map((o) => ({
                value: o.value,
                label: `${t(o.label)}(${o.value})`,
              }))}
              onChange={setSel}
            />
          </label>
          <p className="field__hint">
            {t("转换到归档 / 冷归档层后,对象需先「取回」解冻才能下载。")}
          </p>
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button className="btn btn--primary" onClick={() => onConfirm(sel)}>
            {t("转换")}
          </button>
        </div>
      </div>
    </div>
  );
}

interface RestoreProps {
  name: string;
  onConfirm: (days: number) => void;
  onCancel: () => void;
}

/** 取回归档对象:填写取回后可读的保持天数。 */
export function RestoreDialog({ name, onConfirm, onCancel }: RestoreProps) {
  const { t } = useI18n();
  const [days, setDays] = useState(1);
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("取回归档对象")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("对象:{name}", { name })}</span>
          </label>
          <label className="field">
            <span>{t("取回后可读天数")}</span>
            <input
              type="number"
              min={1}
              value={days}
              onChange={(e) => setDays(Number(e.target.value))}
            />
          </label>
          <p className="field__hint">
            {t("取回是异步的,可能需数分钟到数小时;完成后可在该天数内正常下载。")}
          </p>
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={days < 1}
            onClick={() => onConfirm(days)}
          >
            {t("取回")}
          </button>
        </div>
      </div>
    </div>
  );
}
