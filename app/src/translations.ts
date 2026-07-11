/**
 * 英文翻译表。**key 为中文原文**;英文下查不到的 key 回退中文(渐进覆盖,不会半坏)。
 * 组件里用 `t("中文")` 引用;新增可翻译文案时在此补一行。`{name}` 等为占位符。
 */
export const EN: Record<string, string> = {
  // 通用按钮 / 动作
  取消: "Cancel",
  确定: "OK",
  删除: "Delete",
  创建: "Create",
  保存: "Save",
  关闭: "Close",
  打开: "Open",
  下载: "Download",
  重试: "Retry",
  继续: "Continue",

  // 侧栏
  账号: "Accounts",
  还没有账号: "No accounts yet",
  添加账号: "Add account",
  设置: "Settings",
  关于: "About",
  编辑账号: "Edit account",
  移除账号: "Remove account",
  浅色: "Light",
  深色: "Dark",

  // 工具栏
  "上一层": "Up",
  刷新: "Refresh",
  网格视图: "Grid view",
  列表视图: "List view",
  新建文件夹: "New folder",
  上传文件夹: "Upload folder",
  上传: "Upload",
  "新建 Bucket": "New bucket",
  "在当前目录新建文件夹": "New folder here",
  "上传文件夹到当前目录": "Upload a folder here",
  "上传文件到当前目录": "Upload files here",
  "新建一个 Bucket": "Create a bucket",
  "过滤当前目录…": "Filter this folder…",
  "过滤当前目录 / 回车递归搜索…": "Filter here / Enter to search…",
  "处理中…": "Working…",

  // 列表 / 状态栏
  名称: "Name",
  大小: "Size",
  修改时间: "Modified",
  "加载中…": "Loading…",
  这里空空如也: "Nothing here",
  "全选 / 取消全选": "Select / deselect all",
  "下滑加载更多 · 已显示 {shown} / {total}":
    "Scroll for more · {shown} / {total} shown",
  全部选择: "Select all",
  "{n} 个目录": "{n} folders",
  "{n} 个文件": "{n} files",
  "共 {size}": "{size} total",
  " · 已过滤": " · filtered",
  " · 已选 {n}": " · {n} selected",

  // 批量栏
  "已选 {n} 项": "{n} selected",
  批量下载: "Download",
  转换存储类型: "Change storage class",
  取回归档: "Restore archive",
  批量删除: "Delete",
  取消选择: "Clear selection",

  // 传输面板
  传输: "Transfers",
  " · 进行中 {n}": " · {n} active",
  清除已完成: "Clear finished",
  失败: "Failed",
  已取消: "Cancelled",
  已中断: "Interrupted",
  完成: "Done",
  迁移: "Migrate",
  迁移文件夹: "Migrate folder",

  // 右键菜单
  预览: "Preview",
  详情: "Details",
  校验完整性: "Verify integrity",
  重命名: "Rename",
  "复制 / 移动到": "Copy / move to",
  迁移到其他账号: "Migrate to another account",
  分享链接: "Share link",
  统计信息: "Stats",
  下载文件夹: "Download folder",
  删除文件夹: "Delete folder",
  "删除 Bucket": "Delete bucket",

  // 设置对话框
  "分享链接有效期(分钟)": "Share link expiry (minutes)",
  "批量传输并发数(1–10)": "Concurrent transfers (1–10)",
  "传输限速(KiB/秒,0 = 不限速)": "Transfer rate limit (KiB/s, 0 = unlimited)",
  语言: "Language",
  中文: "中文",
  English: "English",
};
