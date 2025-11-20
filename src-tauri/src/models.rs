use serde::{Deserialize, Serialize};

// 使用泛型 T 代表 data 字段的具体类型
#[derive(Serialize, Deserialize, Debug)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: Option<String>, // 有些接口可能不返回 message，设为 Option
    pub data: Option<T>,
}

// --- 登录相关 ---
#[derive(Serialize, Deserialize, Debug)]
pub struct LoginResponse {
    pub code: i32,
    pub message: String,
    pub data: Option<LoginData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LoginData {
    pub token: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileListData {
    #[serde(rename = "InfoList")]
    pub info_list: Vec<FileInfo>,
    #[serde(rename = "Total")]
    pub total: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileInfo {
    #[serde(rename = "FileId")]
    pub file_id: i64,
    #[serde(rename = "FileName")]
    pub file_name: String,
    #[serde(rename = "Size")]
    pub size: i64,
    #[serde(rename = "Type")]
    pub file_type: i32, // 0: file, 1: folder
    #[serde(rename = "Etag")]
    pub etag: Option<String>,
    #[serde(rename = "S3KeyFlag")]
    pub s3_key_flag: Option<String>,
}

// --- 下载相关 ---
#[derive(Serialize, Deserialize, Debug)]
pub struct DownloadInfoResponse {
    pub code: i32,
    pub message: String,
    pub data: Option<DownloadInfoData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DownloadInfoData {
    #[serde(rename = "DownloadUrl")]
    pub download_url: String,
}

// --- 上传相关 ---
#[derive(Serialize, Deserialize, Debug)]
pub struct UploadRequestResponse {
    pub code: i32,
    pub message: String,
    pub data: Option<UploadRequestData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadRequestData {
    #[serde(rename = "FileId")]
    pub file_id: i64,
    #[serde(rename = "Reuse")]
    pub reuse: bool, // 是否秒传
    #[serde(rename = "UploadId")]
    pub upload_id: Option<String>,
    #[serde(rename = "Key")]
    pub key: Option<String>,
    #[serde(rename = "Bucket")]
    pub bucket: Option<String>,
    #[serde(rename = "StorageNode")]
    pub storage_node: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PresignedUrlResponse {
    pub code: i32,
    pub data: Option<PresignedUrlData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PresignedUrlData {
    #[serde(rename = "presignedUrls")]
    pub presigned_urls: std::collections::HashMap<String, String>,
}

// --- 分享相关 ---
#[derive(Serialize, Deserialize, Debug)]
pub struct ShareResponse {
    pub code: i32,
    pub message: String,
    pub data: Option<ShareData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShareData {
    #[serde(rename = "ShareKey")]
    pub share_key: String,
}

// 用于返回给前端的最终结果
#[derive(Serialize, Deserialize, Debug)]
pub struct ShareResult {
    pub share_url: String,
    pub share_pwd: String,
}
