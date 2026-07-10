//! 用真实腾讯云 COS 账号做一次端到端冒烟:put → head → list → get → delete → multipart。
//!
//! ```bash
//! export COS_SECRET_ID=你的SecretId
//! export COS_SECRET_KEY=你的SecretKey
//! export COS_ENDPOINT=cos.ap-beijing.myqcloud.com   # 你 bucket 所在区域
//! export COS_BUCKET=你的bucket名(含 appid,如 bkt-1250000000)
//! cargo run -p tencent-cos --example tencent_smoke
//! ```

use std::env;

use futures::StreamExt;
use tencent_cos::CosClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sid = env::var("COS_SECRET_ID").expect("需要设置 COS_SECRET_ID");
    let sk = env::var("COS_SECRET_KEY").expect("需要设置 COS_SECRET_KEY");
    let endpoint = env::var("COS_ENDPOINT").expect("需要设置 COS_ENDPOINT");
    let bucket = env::var("COS_BUCKET").expect("需要设置 COS_BUCKET");

    let client = CosClient::new(sid, sk, endpoint);
    let key = "nebula-smoke-test.txt";
    let content = b"hello from nebula tencent-cos smoke test";

    println!("bucket = {bucket}\n");

    {
        let stream = client.list_buckets();
        futures::pin_mut!(stream);
        let mut n = 0;
        while let Some(item) = stream.next().await {
            item?;
            n += 1;
        }
        println!("[0/5] list_buckets  ✅ 账号下 {n} 个 bucket");
    }

    client
        .put_object(&bucket, key, content.to_vec(), Some("text/plain"))
        .await?;
    println!("[1/5] put_object    ✅ 已上传 {} 字节", content.len());

    let meta = client.head_object(&bucket, key).await?;
    println!(
        "[2/5] head_object   ✅ size={} etag={:?}",
        meta.content_length, meta.etag
    );

    let stream = client.list_objects(&bucket, None);
    futures::pin_mut!(stream);
    let mut count = 0usize;
    while let Some(item) = stream.next().await {
        item?;
        count += 1;
    }
    println!("[3/5] list_objects  ✅ 共 {count} 个对象");

    let got = client.get_object(&bucket, key).await?;
    assert_eq!(got.as_ref(), content, "下载内容与上传不一致!");
    println!("[4/5] get_object    ✅ 内容与上传一致");

    client.delete_object(&bucket, key).await?;
    println!("[5/5] delete_object ✅ 已清理临时对象");

    let mp_key = "nebula-smoke-multipart.bin";
    let part_size = tencent_cos::multipart::MIN_PART_SIZE; // 1 MiB
    let big: Vec<u8> = (0..(part_size * 2 + 12345))
        .map(|i| (i % 251) as u8)
        .collect();
    let big_len = big.len();
    client
        .upload_multipart(&bucket, mp_key, big.clone(), part_size, None)
        .await?;
    let back = client.get_object(&bucket, mp_key).await?;
    assert_eq!(back.as_ref(), big.as_slice(), "分片上传后内容不一致");
    client.delete_object(&bucket, mp_key).await?;
    println!(
        "[6/6] multipart     ✅ {big_len} 字节分 {} 片上传/校验/清理完成",
        big_len.div_ceil(part_size)
    );

    println!("\n🎉 全链路验证通过");
    Ok(())
}
