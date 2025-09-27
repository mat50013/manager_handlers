use crate::core::communication::multibus::MultiBus;
use crate::core::{states::SharedState, traits::Base};
use crate::handler;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use std::error::Error;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio_stream::Stream;

handler! {
    DownloadHandler, download;
    async fn run_file(&self,_src: String,  _filename: String) -> Result<(Box<dyn AsyncRead + Send + Unpin>, u64), Box<dyn Error + Send + Sync>> {
        let data = b"test data".to_vec();
        let cursor = Cursor::new(data);
        Ok((Box::new(cursor), 9))
    },
}

handler! {
    TestUpload, upload;
    async fn run_stream(&self,_src: String, mut stream: Pin<Box<dyn Stream<Item = Bytes> + Send>>, _file_name: String, _lower_bound: usize) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut result = String::new();
        while let Some(chunk) = stream.next().await {
            let s = std::str::from_utf8(&chunk)?;
            result.push_str(s);
        }
        Ok(result)
    },
}

handler! {
    UploadHandler, upload;
    async fn run_stream(&self,_src: String, mut stream: Pin<Box<dyn Stream<Item = Bytes> + Send>>, _file_name: String, _lower_bound: usize) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut result = String::new();
        while let Some(chunk) = stream.next().await {
            let s = std::str::from_utf8(&chunk)?;
            result.push_str(s);
        }
        Ok(result)
    },
}

handler! {
    MetadataHandler, metadata;
    async fn run_metadata(&self,_src: String,_file_id: String) -> Result<String, Box<dyn Error + Send + Sync>> {
        Ok("metadata".to_string())
    },
}
