//! 用真实七牛云 Kodo 账号(S3 兼容)做一次端到端冒烟:put → head → list → get → delete → multipart。
//!
//! 通过环境变量传入凭证(不要把密钥写进代码 / 提交进仓库):
//!
//! ```bash
//! export KODO_ACCESS_KEY=你的AK
//! export KODO_SECRET_KEY=你的SK
//! export KODO_ENDPOINT=s3.cn-east-1.qiniucs.com   # 你 bucket 所在区域
//! export KODO_BUCKET=你的bucket名
//! cargo run -p qiniu-kodo --example smoke
//! ```

use std::env;

use futures::StreamExt;
use qiniu_kodo::KodoClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ak = env::var("KODO_ACCESS_KEY").expect("需要设置 KODO_ACCESS_KEY");
    let sk = env::var("KODO_SECRET_KEY").expect("需要设置 KODO_SECRET_KEY");
    let endpoint = env::var("KODO_ENDPOINT").expect("需要设置 KODO_ENDPOINT");
    let bucket = env::var("KODO_BUCKET").expect("需要设置 KODO_BUCKET");

    let client = KodoClient::new(ak, sk, endpoint);
    let key = "nebula-smoke-test.txt";
    let content = b"hello from nebula qiniu-kodo smoke test";

    println!("bucket = {bucket}, region = {}\n", client.region());

    // 0) 列举账号下的 bucket
    {
        let stream = client.list_buckets();
        futures::pin_mut!(stream);
        let mut names = Vec::new();
        while let Some(item) = stream.next().await {
            names.push(item?.name);
        }
        println!(
            "[0/5] list_buckets  ✅ 账号下 {} 个 bucket{}",
            names.len(),
            if names.iter().any(|n| n == &bucket) {
                ",含目标 bucket"
            } else {
                ""
            }
        );
    }

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

    // 3) 列举
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

    // 4) 下载并校验
    let got = client.get_object(&bucket, key).await?;
    assert_eq!(got.as_ref(), content, "下载内容与上传不一致!");
    println!("[4/5] get_object    ✅ 内容与上传一致");

    // 5) 删除
    client.delete_object(&bucket, key).await?;
    println!("[5/5] delete_object ✅ 已清理临时对象");

    // 6) 分片上传(强制多片):造 11MB 数据(每片下限 5MiB)
    let mp_key = "nebula-smoke-multipart.bin";
    let part_size = qiniu_kodo::multipart::MIN_PART_SIZE; // 5 MiB
    let big: Vec<u8> = (0..(part_size * 2 + 12345))
        .map(|i| (i % 251) as u8)
        .collect();
    let big_len = big.len();
    client
        .upload_multipart(
            &bucket,
            mp_key,
            big.clone(),
            part_size,
            Some("application/octet-stream"),
        )
        .await?;
    let back = client.get_object(&bucket, mp_key).await?;
    assert_eq!(back.len(), big_len, "分片上传后大小不一致");
    assert_eq!(back.as_ref(), big.as_slice(), "分片上传后内容不一致");
    client.delete_object(&bucket, mp_key).await?;
    println!(
        "[6/6] multipart     ✅ {big_len} 字节分 {} 片上传/校验/清理完成",
        big_len.div_ceil(part_size)
    );

    println!("\n🎉 全链路验证通过");
    Ok(())
}
