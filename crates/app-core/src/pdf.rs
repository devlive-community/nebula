//! PDF 页面级编辑:后端用 `nebula-pdf`(纯 Rust lopdf)解析与改写,读云端、写回云端。
//!
//! 前端负责 pdf.js 渲染缩略图与交互;所有结构解析 / 组装(删除 / 重排 / 旋转 / 合并 /
//! 提取)都在 Rust 侧完成,大文件也不阻塞界面。

use nebula_pdf::{Assembly, PdfInfo};

use crate::{App, AppError, Result};

impl App {
    /// 读取 PDF 的页面概览(页数、每页尺寸与旋转)。
    pub async fn pdf_info(&self, account: &str, path: &str) -> Result<PdfInfo> {
        let bytes = self.provider(account)?.read(path).await?.to_vec();
        tokio::task::spawn_blocking(move || nebula_pdf::info(&bytes))
            .await
            .map_err(|e| AppError::Image(e.to_string()))?
            .map_err(|e| AppError::Image(e.to_string()))
    }

    /// 取 PDF 原始字节(前端用 pdf.js 渲染缩略图 / 预览)。
    pub async fn pdf_bytes(&self, account: &str, path: &str) -> Result<Vec<u8>> {
        Ok(self.provider(account)?.read(path).await?.to_vec())
    }

    /// 读入主文档 + 合并源,按清单组装,返回输出字节。
    ///
    /// `sources` 是除主文档外要合并进来的其它 PDF 的**原始字节**(与 `Assembly::pages`
    /// 里 `doc` 索引对应:0 = 主文档 `path`,1.. = `sources` 依次)。前端从本地文件选择
    /// 后把字节传进来,无需这些文件在云端。
    async fn pdf_assemble(
        &self,
        account: &str,
        path: &str,
        sources: Vec<Vec<u8>>,
        asm: Assembly,
    ) -> Result<Vec<u8>> {
        let main = self.provider(account)?.read(path).await?.to_vec();
        let mut docs: Vec<Vec<u8>> = Vec::with_capacity(1 + sources.len());
        docs.push(main);
        docs.extend(sources);
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&[u8]> = docs.iter().map(|d| d.as_slice()).collect();
            nebula_pdf::assemble(&refs, &asm)
        })
        .await
        .map_err(|e| AppError::Image(e.to_string()))?
        .map_err(|e| AppError::Image(e.to_string()))
    }

    /// 组装 PDF 并写回云端(`dest == path` 覆盖,否则另存为新对象)。
    pub async fn pdf_save(
        &self,
        account: &str,
        path: &str,
        sources: Vec<Vec<u8>>,
        asm: Assembly,
        dest: &str,
    ) -> Result<()> {
        let out = self.pdf_assemble(account, path, sources, asm).await?;
        self.provider(account)?
            .write(dest, bytes::Bytes::from(out), Some("application/pdf"))
            .await?;
        Ok(())
    }

    /// 组装 PDF 并返回字节(下载到本地用,不写云端)。
    pub async fn pdf_assemble_bytes(
        &self,
        account: &str,
        path: &str,
        sources: Vec<Vec<u8>>,
        asm: Assembly,
    ) -> Result<Vec<u8>> {
        self.pdf_assemble(account, path, sources, asm).await
    }
}
