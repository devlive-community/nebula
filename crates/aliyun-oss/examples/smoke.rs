//! 用真实 OSS 账号做一次端到端冒烟验证:put → head → list → get → delete。
//!
//! 通过环境变量传入凭证(不要把密钥写进代码 / 提交进仓库):
//!
//! ```bash
//! export OSS_ACCESS_KEY_ID=你的AK
//! export OSS_ACCESS_KEY_SECRET=你的SK
//! export OSS_ENDPOINT=oss-cn-hangzhou.aliyuncs.com   # 你 bucket 所在区域
//! export OSS_BUCKET=你的bucket名
//! cargo run -p aliyun-oss --example smoke
//! ```
//!
//! 程序会在 bucket 里创建一个临时对象,验证完自动删除。

use std::env;

use aliyun_oss::OssClient;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ak = env::var("OSS_ACCESS_KEY_ID").expect("需要设置 OSS_ACCESS_KEY_ID");
    let sk = env::var("OSS_ACCESS_KEY_SECRET").expect("需要设置 OSS_ACCESS_KEY_SECRET");
    let endpoint = env::var("OSS_ENDPOINT").expect("需要设置 OSS_ENDPOINT");
    let bucket = env::var("OSS_BUCKET").expect("需要设置 OSS_BUCKET");

    let client = OssClient::new(ak, sk, endpoint);
    let key = "nebula-smoke-test.txt";
    let content = b"hello from nebula aliyun-oss smoke test";

    println!("bucket = {bucket}, key = {key}\n");

    // 1) 上传
    client
        .put_object(&bucket, key, content.to_vec(), Some("text/plain"))
        .await?;
    println!("[1/5] put_object    ✅ 已上传 {} 字节", content.len());

    // 2) 元信息
    let meta = client.head_object(&bucket, key).await?;
    println!(
        "[2/5] head_object   ✅ size={} type={:?} etag={:?}",
        meta.content_length, meta.content_type, meta.etag
    );

    // 3) 列举(打印前若干个)
    let stream = client.list_objects(&bucket, None);
    futures::pin_mut!(stream);
    let mut count = 0usize;
    let mut saw_key = false;
    while let Some(item) = stream.next().await {
        let obj = item?;
        if obj.key == key {
            saw_key = true;
        }
        if count < 5 {
            println!("        · {} ({} bytes)", obj.key, obj.size);
        }
        count += 1;
    }
    println!(
        "[3/5] list_objects  ✅ 共 {count} 个对象,{}包含刚上传的 key",
        if saw_key { "" } else { "⚠️ 未" }
    );

    // 4) 下载并校验内容
    let got = client.get_object(&bucket, key).await?;
    assert_eq!(got.as_ref(), content, "下载内容与上传不一致!");
    println!("[4/5] get_object    ✅ 内容与上传一致");

    // 5) 删除
    client.delete_object(&bucket, key).await?;
    println!("[5/5] delete_object ✅ 已清理临时对象");

    println!("\n🎉 全链路验证通过");
    Ok(())
}
