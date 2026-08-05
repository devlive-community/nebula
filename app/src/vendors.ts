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
  tencent: {
    label: "腾讯云 COS",
    color: "#006eff",
    akLabel: "SecretId",
    skLabel: "SecretKey",
    endpoint: "cos.ap-beijing.myqcloud.com",
    idPlaceholder: "如 tencent-main",
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
  r2: {
    label: "Cloudflare R2",
    color: "#f6821f",
    akLabel: "Access Key ID",
    skLabel: "Secret Access Key",
    endpoint: "<account_id>.r2.cloudflarestorage.com",
    idPlaceholder: "如 r2-main",
  },
  minio: {
    label: "MinIO",
    color: "#c72e49",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "http://minio.example.com:9000",
    idPlaceholder: "如 minio-main",
  },
  b2: {
    label: "Backblaze B2",
    color: "#e21b24",
    akLabel: "Key ID",
    skLabel: "Application Key",
    endpoint: "s3.<region>.backblazeb2.com",
    idPlaceholder: "如 b2-main",
  },
  wasabi: {
    label: "Wasabi",
    color: "#22c02e",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "s3.<region>.wasabisys.com",
    idPlaceholder: "如 wasabi-main",
  },
  do_spaces: {
    label: "DigitalOcean Spaces",
    color: "#0069ff",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "<region>.digitaloceanspaces.com",
    idPlaceholder: "如 do-spaces-main",
  },
  scaleway: {
    label: "Scaleway Object Storage",
    color: "#4f0599",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "s3.<region>.scw.cloud",
    idPlaceholder: "如 scaleway-main",
  },
  us3: {
    label: "UCloud US3",
    color: "#2e5bff",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "s3-<region>.ufileos.com",
    idPlaceholder: "如 us3-main",
  },
  jdcloud: {
    label: "京东云 OSS",
    color: "#e3101e",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "s3.cn-south-1.jdcloud-oss.com",
    idPlaceholder: "如 jdcloud-main",
  },
  upyun: {
    label: "又拍云",
    color: "#2ac845",
    akLabel: "Access Key",
    skLabel: "Secret Key",
    endpoint: "s3.api.upyun.com",
    idPlaceholder: "如 upyun-main",
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
