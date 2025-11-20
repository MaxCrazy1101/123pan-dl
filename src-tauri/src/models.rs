use serde::{Deserialize, Serialize};

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

// --- 文件列表相关 ---
#[derive(Serialize, Deserialize, Debug)]
pub struct FileListResponse {
    pub code: i32,
    pub data: Option<FileListData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileListData {
    #[serde(rename = "InfoList")]
    pub info_list: Vec<FileInfo>,
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
    pub s3_key_flag: Option<String>, // 下载必须字段
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
