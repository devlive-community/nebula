---
title: 一个 XML 反序列化库的隐藏假设:同名重复元素必须挨在一起
date: 2026-08-03
author: Nebula Team
description: 给对象加版本历史,读版本列表时踩到一个不容易想到的坑——响应里的 Version 和 DeleteMarker 是按时间交错排列的,而 XML 反序列化库在这种交错下会直接报错，不是"顺序乱了"这么简单。
tags: ['开发', '对象存储', '架构']
---

Nebula 这次加了**对象版本控制**:Bucket 开关(启用/暂停)+ 单个对象的版本历史(列版本、恢复旧版本、处理删除标记)。整体骨架和上一篇「生命周期规则」完全同构——trait 默认方法 + 能力位、四份 SDK 实现(阿里云/华为云/腾讯云各自独立,AWS/R2/MinIO 共用 s3-core)、七牛云本轮继续标记不支持。这篇只记一个新踩到的坑:**解析版本列表的 XML 时,一个"看起来很基础"的反序列化库假设翻车了**。

## 响应里,新旧版本和删除标记是交错的

`ListObjectVersions` 的响应大致长这样:

```xml
<ListVersionsResult>
  <Version><Key>a.txt</Key><VersionId>v3</VersionId><IsLatest>false</IsLatest>...</Version>
  <DeleteMarker><Key>a.txt</Key><VersionId>v2</VersionId><IsLatest>false</IsLatest>...</DeleteMarker>
  <Version><Key>a.txt</Key><VersionId>v1</VersionId><IsLatest>false</IsLatest>...</Version>
</ListVersionsResult>
```

`Version` 和 `DeleteMarker` **按时间交错**,这是真实语义:一个文件可能被删除又恢复又删除,每次操作都在同一个 key 的历史里插一条,谁在前谁在后完全看操作顺序,不会乖乖分成两堆。

第一版实现很自然:一个 struct 两个字段,分别收集两种元素:

```rust
#[derive(Deserialize)]
struct ListVersionsResultXml {
    #[serde(default, rename = "Version")]
    version: Vec<VersionXml>,
    #[serde(default, rename = "DeleteMarker")]
    delete_marker: Vec<DeleteMarkerXml>,
}
```

拿真实交错的 XML 一测,直接报错:`duplicate field "Version"`。

## 不是"顺序丢了",是"压根解析不出来"

一开始以为只是"反序列化完两个 Vec 后顺序对不上",打算解析完再按时间重新排序——但错误是硬报错,不是"结果不对"。

`quick_xml` 的 serde 支持在处理"一个 struct 里两个重复元素字段"时,内部逻辑大致是:遇到 `<Version>` 就往 `version` 这个累加器里塞,遇到一个不认识的兄弟标签就跳过继续找下一个能匹配的字段。**但一旦中间被"跳过"打断过,再遇到 `<Version>` 时,反序列化器已经认为这个字段"关闭"过了,当作重复字段报错**——即使中间那个"打断者"是完全不相关、会被忽略的 `DeleteMarker`。换句话说,这个限制不是"两个字段互相干扰",而是"同名重复元素必须严格挨在一起,任何东西insert在中间都不行",哪怕插进来的东西这个 struct 根本不关心。

先试了个更省事的招:干脆拆两次解析,一次只声明 `version` 字段(把 `DeleteMarker` 当"不认识的字段"直接忽略),另一次反过来。想法是"既然对方压根不在 struct 里,总不会被计入累加器了吧"——结果**同一个错误依然出现**:哪怕目标 struct 完全不认识 `DeleteMarker` 这个标签,只要它出现在两个 `Version` 之间,`Version` 这个 Vec 字段照样被判定为"提前关闭",第二次遇到照样报重复。这说明问题出在"重复元素本身连续与否",跟 struct 认不认识插进来的东西无关。

## 解法:别指望 struct 派生,自己控制读到哪一步

既然"整个响应体一次性反序列化成一个 struct"这条路走不通,那就退一步,变成"逐个顶层子元素反序列化",和这个仓库到处在用的"整个响应体就是一个对象"完全是同一种代码,只是外面套一层手动的事件流扫描:

```rust
let mut reader = Reader::from_str(xml);
loop {
    match reader.read_event_into(&mut buf)? {
        Event::Start(e) if e.name() == QName(b"Version") => {
            let span = reader.read_to_end_into(QName(b"Version"), &mut Vec::new())?;
            let inner = &xml[span.start as usize..span.end as usize];
            let v: VersionXml = quick_xml::de::from_str(&format!("<Version>{inner}</Version>"))?;
            // ...塞进结果里
        }
        Event::Start(e) if e.name() == QName(b"DeleteMarker") => { /* 同理 */ }
        Event::Eof => break,
        _ => {}
    }
}
```

`Reader::read_to_end_into` 拿到的是标签内部的字节范围,重新包一层根标签再丢给 `quick_xml::de::from_str`——这时候每次反序列化的对象都是自包含的一个 `<Version>...</Version>`,不存在"和谁挨着"的问题,交错多少次都无所谓。拿到全部版本后按 `last_modified` 重新排一次序,得到从新到旧的列表。

## 顺手解决的另一件事:「恢复」和「撤销删除」原来是同一个操作

版本历史面板设计成三种行为:「恢复某个旧版本」「删除某个具体版本(不可撤销)」——但**没有**单独的「撤销删除」按钮。这是刻意的:S3 语义里,删除一个开了版本控制的对象,并不是真删,只是在历史顶端插了一条 `DeleteMarker`;想要"复活"这个 key,只需要把这条 `DeleteMarker` 当成一个普通版本**永久删除掉**,前一个真实版本就自动变回当前版本了。所以面板上删除标记那一行只有一个「永久删除」按钮,点了它,效果就是撤销删除——不需要专门写一个"撤销删除"的后端方法。

「恢复某个旧版本」则是另一件事:服务端自我复制,把旧版本的内容重新 PUT 一遍,复制源带上 `?versionId=`。这和上一轮"转换存储类型"用的自我复制签名路径是同一条,只是 `x-{oss,obs,cos,amz}-copy-source` 头里多了个版本号:

```rust
let copy_source = format!("/{bucket}/{}?versionId={version_id}", encode_key(key));
```

## 小结

这次的教训不是"XML 解析要小心",而是更具体的一条:**遇到"同名元素在真实数据里会交错出现"的场景,先假设你的反序列化库的重复字段支持是按"连续分组"设计的,直到验证过为止**。struct 派生反序列化的心智模型通常是"给我一整份数据,我按字段名分组",一旦真实世界的顺序不满足"同名元素总是连续"这个隐藏假设,报错往往比想象中更直接、更难一眼看穿原因——这时候退回到手动的、逐元素扫描的解析方式,反而比硬凑一个更复杂的 `#[serde]` 标注组合更可靠。
