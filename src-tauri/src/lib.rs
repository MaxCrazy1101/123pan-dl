use futures_util::StreamExt;
use log::{debug, error, info, warn};
use md5::{Digest, Md5};
use regex::Regex;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write}; // 用于文件分块读取
use std::sync::Mutex;
use tauri::{Emitter, State, Window};
use tauri_plugin_store::StoreExt;
use tokio::io::AsyncReadExt;
use uuid::Uuid; // 异步读取

mod models;
use models::*;

pub struct AppState {
    client: Client,
    token: Mutex<String>,
    login_uuid: String,
}

impl AppState {
    fn new() -> Self {
        let client = Client::builder()
            .cookie_store(true)
            .user_agent("123pan/v2.4.0(Android_7.1.2;Xiaomi)")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .unwrap();

        let login_uuid = Uuid::new_v4().simple().to_string();

        Self {
            client,
            token: Mutex::new(String::new()),
            login_uuid,
        }
    }
}

fn add_auth_headers(request: RequestBuilder, token: &str, login_uuid: &str) -> RequestBuilder {
    request
        .header("authorization", token)
        .header("platform", "android")
        .header("app-version", "61")
        .header("x-app-version", "2.4.0")
        .header("x-channel", "1004")
        .header("devicetype", "M2101K9C")
        .header("devicename", "Xiaomi")
        .header("osversion", "Android_7.1.2")
        .header("loginuuid", login_uuid)
        .header("content-type", "application/json")
}

#[derive(Serialize, Deserialize)]
struct Credentials {
    username: String,
    password: String,
    token: Option<String>,
}

#[tauri::command]
async fn login(
    app: tauri::AppHandle,
    username: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!("尝试登录用户: {}", username);

    let url = "https://www.123pan.com/b/api/user/sign_in";
    let payload = json!({
        "type": 1,
        "passport": username,
        "password": password
    });

    let req = state.client.post(url).json(&payload);
    let req = add_auth_headers(req, "", &state.login_uuid);

    let res = req.send().await.map_err(|e| {
        error!("登录网络请求失败: {}", e);
        e.to_string()
    })?;

    let json_res: LoginResponse = res.json().await.map_err(|e| e.to_string())?;

    if json_res.code != 200 {
        warn!("登录失败，服务器返回: {}", json_res.message);
        return Err(json_res.message);
    }

    if let Some(data) = json_res.data {
        let token_str = format!("Bearer {}", data.token);
        let mut token = state.token.lock().unwrap();
        *token = token_str.clone();

        let store = app.store("auth.json").map_err(|e| e.to_string())?;
        store.set(
            "credentials",
            json!({
                "username": username,
                "password": password,
                "token": token_str
            }),
        );
        store.save().map_err(|e| e.to_string())?;

        info!("登录成功并已保存凭证");
        return Ok("登录成功".to_string());
    }

    Err("未知登录错误".to_string())
}

// 获取文件列表
#[tauri::command]
async fn get_file_list(
    parent_file_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    debug!("正在获取目录列表: {}", parent_file_id);

    let url = "https://www.123pan.com/b/api/file/list/new";
    let token = state.token.lock().unwrap().clone();

    let mut all_files: Vec<FileInfo> = Vec::new();
    let mut page = 1;
    let mut total_files = -1;
    let mut fetched_count = 0;

    // 循环获取分页
    loop {
        // 如果已经获取了所有文件，跳出循环
        if total_files != -1 && fetched_count >= total_files {
            break;
        }

        let params = [
            ("driveId", "0"),
            ("limit", "100"),
            ("next", "0"),
            ("orderBy", "file_id"),
            ("orderDirection", "desc"),
            ("parentFileId", &parent_file_id.to_string()),
            ("trashed", "false"),
            ("SearchData", ""),
            ("Page", &page.to_string()),
            ("OnlyLookAbnormalFile", "0"),
        ];

        let req = state.client.get(url).query(&params);
        let req = add_auth_headers(req, &token, &state.login_uuid);

        let res = req.send().await.map_err(|e| e.to_string())?;
        let json_res: ApiResponse<FileListData> = res.json().await.map_err(|e| e.to_string())?;

        if json_res.code != 0 {
            // 优化：尝试获取服务器返回的错误消息
            let msg = json_res.message.unwrap_or_else(|| "未知错误".to_string());
            error!("获取列表失败 Code: {}, Msg: {}", json_res.code, msg);
            return Err(format!("获取列表失败: {} (Code: {})", msg, json_res.code));
        }

        if let Some(data) = json_res.data {
            // 初始化总数
            if total_files == -1 {
                total_files = data.total.unwrap_or(0);
            }

            let page_count = data.info_list.len() as i64;
            if page_count == 0 {
                break;
            }

            all_files.extend(data.info_list);
            fetched_count += page_count;
            page += 1;
        } else {
            break;
        }
    }

    debug!("共获取到 {} 个文件", all_files.len());
    Ok(all_files)
}

#[tauri::command]
async fn try_auto_login(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let store = app.store("auth.json").map_err(|e| e.to_string())?;

    if let Some(value) = store.get("credentials") {
        let creds: Credentials =
            serde_json::from_value(value.clone()).map_err(|_| "凭证格式错误".to_string())?;

        // 策略 A: 验证旧 Token
        if let Some(saved_token) = creds.token {
            let check_url = "https://www.123pan.com/b/api/file/list/new";
            let params = [
                ("driveId", "0"),
                ("limit", "1"),
                ("next", "0"),
                ("orderBy", "file_id"),
                ("orderDirection", "desc"),
                ("parentFileId", "0"),
                ("trashed", "false"),
                ("SearchData", ""),
                ("Page", "1"),
                ("OnlyLookAbnormalFile", "0"),
            ];

            let req = state.client.get(check_url).query(&params);
            let req = add_auth_headers(req, &saved_token, &state.login_uuid);

            let res = req.send().await;

            if let Ok(response) = res {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    if json.get("code").and_then(|c| c.as_i64()) == Some(0) {
                        let mut token_lock = state.token.lock().unwrap();
                        *token_lock = saved_token;
                        info!("自动登录：Token 有效，复用成功");
                        return Ok(true);
                    } else {
                        warn!("自动登录：Token 校验失败，API 返回: {:?}", json);
                    }
                }
            }
        }

        // 策略 B: 重新登录
        info!("自动登录：Token 失效或校验未通过，使用密码重新登录...");

        let url = "https://www.123pan.com/b/api/user/sign_in";
        let payload = json!({"type": 1, "passport": creds.username, "password": creds.password});

        let req = state.client.post(url).json(&payload);
        let req = add_auth_headers(req, "", &state.login_uuid);

        let res = req.send().await.map_err(|e| e.to_string())?;
        let json_res: LoginResponse = res.json().await.map_err(|e| e.to_string())?;

        if json_res.code == 200 {
            if let Some(data) = json_res.data {
                let new_token_str = format!("Bearer {}", data.token);
                let mut token_lock = state.token.lock().unwrap();
                *token_lock = new_token_str.clone();

                store.set(
                    "credentials",
                    json!({
                        "username": creds.username,
                        "password": creds.password,
                        "token": new_token_str
                    }),
                );
                store
                    .save()
                    .map_err(|e| format!("保存 Token 失败: {}", e))?;
                info!("自动登录：密码重登成功");
                return Ok(true);
            }
        } else {
            error!("自动登录：密码重登失败 Code: {}", json_res.code);
        }
    }
    Ok(false)
}

#[tauri::command]
async fn logout(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    info!("用户执行退出登录");
    let store = app.store("auth.json").map_err(|e| e.to_string())?;
    store.delete("credentials");
    store.save().map_err(|e| e.to_string())?;

    let mut token = state.token.lock().unwrap();
    *token = String::new();

    Ok(())
}

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    id: String,
    progress: u64,
    speed: String,
    status: String,
}

#[tauri::command]
async fn download_file(
    file_id: i64,
    file_name: String,
    file_type: i32,
    etag: String,
    s3_key_flag: String,
    size: i64,
    save_path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("开始下载: {} (Type: {})", file_name, file_type);

    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    // 步骤 1: 根据类型选择 API 和 Payload
    let intermediate_url;

    if file_type == 1 {
        // --- 文件夹逻辑 ---
        let batch_url = "https://www.123pan.com/a/api/file/batch_download_info";
        let payload = json!({
            "fileIdList": [{ "fileId": file_id }]
        });

        let req = client.post(batch_url).json(&payload);
        let req = add_auth_headers(req, &token, &state.login_uuid);
        let res = req.send().await.map_err(|e| e.to_string())?;
        let info_res: DownloadInfoResponse = res.json().await.map_err(|e| e.to_string())?;

        if info_res.code != 0 {
            return Err(format!("获取打包下载链接失败: {}", info_res.message));
        }
        intermediate_url = info_res.data.map(|d| d.download_url).ok_or("链接为空")?;
    } else {
        // --- 单文件逻辑 ---
        let info_url = "https://www.123pan.com/a/api/file/download_info";
        let payload = json!({
            "driveId": 0,
            "fileId": file_id,
            "etag": etag,
            "s3keyFlag": s3_key_flag,
            "type": 0,
            "fileName": file_name,
            "size": size
        });

        let req = client.post(info_url).json(&payload);
        let req = add_auth_headers(req, &token, &state.login_uuid);
        let res = req.send().await.map_err(|e| e.to_string())?;
        let info_res: DownloadInfoResponse = res.json().await.map_err(|e| e.to_string())?;

        if info_res.code != 0 {
            return Err(format!("获取下载链接失败: {}", info_res.message));
        }
        intermediate_url = info_res.data.map(|d| d.download_url).ok_or("链接为空")?;
    }

    // 步骤 2: 解析中间页 (复用已有逻辑)
    let no_redirect_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("123pan/v2.4.0(Android_7.1.2;Xiaomi)")
        .build()
        .map_err(|e| e.to_string())?;

    let html_res = no_redirect_client
        .get(&intermediate_url)
        .send()
        .await
        .map_err(|e| format!("中间页请求失败: {}", e))?;

    // 检查 Location 头
    let final_download_url = if let Some(loc) = html_res.headers().get("location") {
        loc.to_str().unwrap_or_default().to_string()
    } else {
        let html_text = html_res
            .text()
            .await
            .map_err(|e| format!("读取跳转页失败: {}", e))?;
        let re = Regex::new(r"href='(https?://[^']+)'").map_err(|e| format!("正则错误: {}", e))?;
        re.captures(&html_text)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
            .ok_or("无法解析下载地址")?
    };

    // 步骤 3: 真实下载
    let res = client
        .get(&final_download_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // 注意：文件夹打包下载时，API 返回的 Size 可能是 0 或者不准确
    // 我们优先使用 response header 中的 Content-Length，如果也没有，则无法计算进度
    let total_size = res
        .content_length()
        .unwrap_or(if size > 0 { size as u64 } else { 0 });

    let mut stream = res.bytes_stream();
    let mut file = File::create(&save_path).map_err(|e| format!("创建文件失败: {}", e))?;
    let mut downloaded: u64 = 0;

    window
        .emit(
            "download-progress",
            ProgressPayload {
                id: file_id.to_string(),
                progress: 0,
                speed: "".to_string(),
                status: "downloading".to_string(),
            },
        )
        .unwrap_or(());

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("下载流中断: {}", e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("写入失败: {}", e))?;

        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let percent = (downloaded * 100) / total_size;
            window
                .emit(
                    "download-progress",
                    ProgressPayload {
                        id: file_id.to_string(),
                        progress: percent,
                        speed: "".to_string(),
                        status: "downloading".to_string(),
                    },
                )
                .unwrap_or(());
        }
    }

    info!("文件下载完成: {}", file_name);

    window
        .emit(
            "download-progress",
            ProgressPayload {
                id: file_id.to_string(),
                progress: 100,
                speed: "".to_string(),
                status: "finished".to_string(),
            },
        )
        .unwrap_or(());

    Ok(())
}

async fn calculate_file_md5(file_path: String) -> Result<(String, u64), String> {
    let path_clone = file_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(String, u64), String> {
        let mut file = File::open(&path_clone).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut hasher = Md5::new();
        let mut buffer = [0; 8192]; // 8KB buffer

        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok((hex::encode(hasher.finalize()), len))
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(result)
}

// 上传进度事件
#[derive(Clone, serde::Serialize)]
struct UploadProgressPayload {
    id: String, // path
    progress: u64,
    status: String, // "hashing", "uploading", "finished", "error"
}

#[tauri::command]
async fn upload_file(
    parent_file_id: i64,
    file_path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    // 获取文件名
    let path_obj = std::path::Path::new(&file_path);
    let file_name = path_obj
        .file_name()
        .ok_or("无效的文件路径")?
        .to_str()
        .ok_or("文件名包含非UTF-8字符")?
        .to_string();

    // 1. 计算 MD5
    window
        .emit(
            "upload-progress",
            UploadProgressPayload {
                id: file_path.clone(),
                progress: 0,
                status: "hashing".to_string(),
            },
        )
        .unwrap_or(());

    info!("正在计算文件 MD5: {}", file_name);
    let (etag, size) = calculate_file_md5(file_path.clone()).await?;

    // 2. 发起上传请求 (Upload Request)
    let request_url = "https://www.123pan.com/b/api/file/upload_request";

    // 定义一个闭包来构造 Payload，方便重试
    let create_payload = |dup_policy: i32| {
        json!({
            "driveId": 0,
            "etag": etag,
            "fileName": file_name,
            "parentFileId": parent_file_id,
            "size": size,
            "type": 0,
            "duplicate": dup_policy // 0: 询问, 1: 覆盖, 2: 重命名
        })
    };

    // 第一次尝试，默认 duplicate=0
    let mut req = client.post(request_url).json(&create_payload(0));
    req = add_auth_headers(req, &token, &state.login_uuid);

    let res = req.send().await.map_err(|e| e.to_string())?;
    let mut json_res: UploadRequestResponse = res.json().await.map_err(|e| e.to_string())?;

    // 如果返回 5060 (文件已存在)，自动选择重命名 (duplicate=2) 并重试
    if json_res.code == 5060 {
        info!("文件已存在，尝试自动重命名...");
        let req_retry = client.post(request_url).json(&create_payload(2)); // 2 = Rename
        let req_retry = add_auth_headers(req_retry, &token, &state.login_uuid);
        let res_retry = req_retry.send().await.map_err(|e| e.to_string())?;
        json_res = res_retry.json().await.map_err(|e| e.to_string())?;
    }

    if json_res.code != 0 {
        return Err(format!("上传请求拒绝: {}", json_res.message));
    }

    let data = json_res.data.ok_or("API 未返回数据")?;

    // 3. 检查是否秒传
    if data.reuse {
        info!("秒传成功: {}", file_name);
        window
            .emit(
                "upload-progress",
                UploadProgressPayload {
                    id: file_path.clone(),
                    progress: 100,
                    status: "finished".to_string(),
                },
            )
            .unwrap_or(());
        return Ok(());
    }

    // 4. 准备分块上传 S3
    let upload_id = data.upload_id.ok_or("缺少 UploadId")?;
    let key = data.key.ok_or("缺少 Key")?;
    let bucket = data.bucket.ok_or("缺少 Bucket")?;
    let storage_node = data.storage_node.unwrap_or_default();
    let file_id_server = data.file_id; // 最终完成时需要

    // 初始化 S3 上传列表 (Python 源码逻辑)
    let init_s3_url = "https://www.123pan.com/b/api/file/s3_list_upload_parts";
    let s3_base_payload = json!({
        "bucket": bucket,
        "key": key,
        "uploadId": upload_id,
        "storageNode": storage_node
    });

    let req_init = client.post(init_s3_url).json(&s3_base_payload);
    let req_init = add_auth_headers(req_init, &token, &state.login_uuid);
    req_init.send().await.map_err(|e| e.to_string())?;

    // 5. 循环分块上传
    let block_size: u64 = 5 * 1024 * 1024; // 5MB
    let mut file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|e| e.to_string())?;
    let mut part_number = 1;
    let mut uploaded_bytes: u64 = 0;

    loop {
        // 读取 5MB 数据
        let mut buffer = vec![0u8; block_size as usize];
        let n = file.read(&mut buffer).await.map_err(|e| e.to_string())?;

        if n == 0 {
            break;
        } // EOF

        // 截断 buffer 到实际读取大小 (最后一块可能小于 5MB)
        buffer.truncate(n);

        // 获取分块上传链接
        let get_url_api = "https://www.123pan.com/b/api/file/s3_repare_upload_parts_batch";
        let url_payload = json!({
            "bucket": bucket,
            "key": key,
            "uploadId": upload_id,
            "storageNode": storage_node,
            "partNumberStart": part_number,
            "partNumberEnd": part_number + 1
        });

        let req_url = client.post(get_url_api).json(&url_payload);
        let req_url = add_auth_headers(req_url, &token, &state.login_uuid);
        let res_url = req_url.send().await.map_err(|e| e.to_string())?;
        let json_url: PresignedUrlResponse = res_url.json().await.map_err(|e| e.to_string())?;

        if json_url.code != 0 {
            return Err("获取上传链接失败".to_string());
        }

        let presigned_url = json_url
            .data
            .and_then(|d| d.presigned_urls.get(&part_number.to_string()).cloned())
            .ok_or("未找到对应分块的上传链接")?;

        // PUT 数据到 S3 (使用不带 Auth Header 的请求)
        client
            .put(&presigned_url)
            .body(buffer) // 直接发送二进制
            .send()
            .await
            .map_err(|e| format!("分块 {} 上传失败: {}", part_number, e))?;

        // 更新进度
        uploaded_bytes += n as u64;
        part_number += 1;

        let percent = (uploaded_bytes * 100) / size;
        window
            .emit(
                "upload-progress",
                UploadProgressPayload {
                    id: file_path.clone(),
                    progress: percent,
                    status: "uploading".to_string(),
                },
            )
            .unwrap_or(());
    }

    // 6. 完成上传
    // 发送 S3 完成信号
    let complete_s3_url = "https://www.123pan.com/b/api/file/s3_complete_multipart_upload";
    let req_comp_s3 = client.post(complete_s3_url).json(&s3_base_payload);
    let req_comp_s3 = add_auth_headers(req_comp_s3, &token, &state.login_uuid);
    req_comp_s3
        .send()
        .await
        .map_err(|e| format!("S3 完成信号发送失败: {}", e))?;

    // 发送业务完成信号
    let complete_api_url = "https://www.123pan.com/b/api/file/upload_complete";
    let req_comp_api = client
        .post(complete_api_url)
        .json(&json!({ "fileId": file_id_server }));
    let req_comp_api = add_auth_headers(req_comp_api, &token, &state.login_uuid);
    let res_final = req_comp_api.send().await.map_err(|e| e.to_string())?;

    // 检查最终结果
    let status = res_final.status();
    if !status.is_success() {
        return Err(format!("服务器返回错误状态: {}", status));
    }

    info!("上传流程结束: {}", file_name);
    window
        .emit(
            "upload-progress",
            UploadProgressPayload {
                id: file_path.clone(),
                progress: 100,
                status: "finished".to_string(),
            },
        )
        .unwrap_or(());

    Ok(())
}
// 新建文件夹
#[tauri::command]
async fn create_folder(
    parent_file_id: i64,
    folder_name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("尝试创建文件夹: {}", folder_name);
    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    let url = "https://www.123pan.com/a/api/file/upload_request";

    // 构造新建文件夹的特殊 payload (参考 Python 逻辑)
    let payload = json!({
        "driveId": 0,
        "etag": "",
        "fileName": folder_name,
        "parentFileId": parent_file_id,
        "size": 0,
        "type": 1,
        "duplicate": 1,
        "NotReuse": true,
        "event": "newCreateFolder",
        "operateType": 1
    });

    let req = client.post(url).json(&payload);
    let req = add_auth_headers(req, &token, &state.login_uuid);

    let res = req.send().await.map_err(|e| e.to_string())?;

    let json_res: ApiResponse<serde_json::Value> = res
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    if json_res.code != 0 {
        let msg = json_res.message.unwrap_or_else(|| "未知错误".to_string());
        return Err(msg);
    }

    info!("创建文件夹成功");
    Ok(())
}

// 6. 删除文件 (新增功能)
#[tauri::command]
async fn delete_file(file_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    info!("尝试删除文件 ID: {}", file_id);
    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    let url = "https://www.123pan.com/a/api/file/trash";

    // 123Pan 删除接口要求传入一个数组，这里我们只删一个
    let payload = json!({
        "driveId": 0,
        "fileTrashInfoList": [
            { "fileId": file_id }
        ],
        "operation": true // true = 删除(移入回收站), false = 恢复
    });

    let req = client.post(url).json(&payload);
    let req = add_auth_headers(req, &token, &state.login_uuid);

    let res = req.send().await.map_err(|e| e.to_string())?;
    let json_res: ApiResponse<serde_json::Value> = res
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    if json_res.code != 0 {
        let msg = json_res.message.unwrap_or_else(|| "未知错误".to_string());
        return Err(msg);
    }

    info!("删除文件成功");
    Ok(())
}

#[tauri::command]
async fn share_file(
    file_ids: Vec<i64>,
    share_pwd: Option<String>,
    state: State<'_, AppState>,
) -> Result<ShareResult, String> {
    info!("尝试分享文件: {:?}", file_ids);
    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    if file_ids.is_empty() {
        return Err("未选择文件".to_string());
    }
    let file_id_list_str = file_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<String>>()
        .join(",");
    let pwd = share_pwd.unwrap_or_default();

    let url = "https://www.123pan.com/a/api/share/create";
    let payload = json!({
        "driveId": 0,
        "expiration": "2099-12-12T08:00:00+08:00",
        "fileIdList": file_id_list_str,
        "shareName": "My Share",
        "sharePwd": pwd,
        "event": "shareCreate"
    });

    let req = client.post(url).json(&payload);
    let req = add_auth_headers(req, &token, &state.login_uuid);
    let res = req.send().await.map_err(|e| e.to_string())?;
    let json_res: ShareResponse = res.json().await.map_err(|e| e.to_string())?;

    if json_res.code != 0 {
        return Err(json_res.message);
    }

    let key = json_res.data.ok_or("API 未返回 ShareKey")?.share_key;
    Ok(ShareResult {
        share_url: format!("https://www.123pan.com/s/{}", key),
        share_pwd: pwd,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .max_file_size(1024 * 1024 * 2)
                .level(log::LevelFilter::Info)
                .build(),
        )
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            login,
            get_file_list,
            download_file,
            try_auto_login,
            logout,
            create_folder,
            delete_file,
            upload_file,
            share_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
