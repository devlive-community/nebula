/** 各云厂商的展示元数据(表单默认值 + 侧栏图标品牌色)。新增厂商在此追加一项即可。 */
export const VENDORS = {
  aliyun: {
    label: "阿里云 OSS",
    color: "#ff6a00",
    akLabel: "AccessKeyId",
    skLabel: "AccessKeySecret",
    endpoint: "oss-cn-hangzhou.aliyuncs.com",
    idPlaceholder: "如 aliyun-main",
  },
  huawei: {
    label: "华为云 OBS",
    color: "#c7000b",
    akLabel: "AccessKey(AK)",
    skLabel: "SecretKey(SK)",
    endpoint: "obs.cn-north-4.myhuaweicloud.com",
    idPlaceholder: "如 huawei-main",
  },
  qiniu: {
    label: "七牛云 Kodo",
    color: "#12b5a5",
    akLabel: "AccessKey(AK)",
    skLabel: "SecretKey(SK)",
    endpoint: "s3.cn-east-1.qiniucs.com",
    idPlaceholder: "如 qiniu-main",
  },
  aws: {
    label: "AWS S3",
    color: "#ff9900",
    akLabel: "Access Key ID",
    skLabel: "Secret Access Key",
    endpoint: "s3.us-east-1.amazonaws.com",
    idPlaceholder: "如 aws-main",
  },
} as const;

export type Vendor = keyof typeof VENDORS;

/** 取厂商元数据,未知厂商回退到一份中性默认值。 */
export function vendorMeta(vendor: string) {
  return (
    VENDORS[vendor as Vendor] ?? {
      label: vendor,
      color: "var(--text-dim)",
      akLabel: "AccessKey",
      skLabel: "SecretKey",
      endpoint: "",
      idPlaceholder: "",
    }
  );
}
