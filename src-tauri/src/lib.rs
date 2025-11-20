use futures_util::StreamExt;
use log::{debug, error, info, warn};
use regex::Regex;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use tauri::State;
use tauri::{Emitter, Window};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

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

// 补全所有 Header
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

// 2. 获取文件列表
#[tauri::command]
async fn get_file_list(
    parent_file_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    debug!("正在获取目录列表: {}", parent_file_id);

    let url = "https://www.123pan.com/b/api/file/list/new";
    let token = state.token.lock().unwrap().clone();

    let params = [
        ("driveId", "0"),
        ("limit", "100"),
        ("next", "0"),
        ("orderBy", "file_id"),
        ("orderDirection", "desc"),
        ("parentFileId", &parent_file_id.to_string()),
        ("trashed", "false"),
        ("SearchData", ""),
        ("Page", "1"),
        ("OnlyLookAbnormalFile", "0"),
    ];

    let req = state.client.get(url).query(&params);
    let req = add_auth_headers(req, &token, &state.login_uuid);

    let res = req.send().await.map_err(|e| e.to_string())?;
    let json_res: FileListResponse = res.json().await.map_err(|e| e.to_string())?;

    if json_res.code != 0 {
        error!("获取列表失败 Code: {}", json_res.code);
        return Err(format!("获取列表失败 Code: {}", json_res.code));
    }

    Ok(json_res.data.map(|d| d.info_list).unwrap_or_default())
}

// 3. 自动登录
#[tauri::command]
async fn try_auto_login(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let store = app.store("auth.json").map_err(|e| e.to_string())?;

    if let Some(value) = store.get("credentials") {
        let creds: Credentials =
            serde_json::from_value(value.clone()).map_err(|_| "凭证格式错误".to_string())?;

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
    debug!("用户执行退出登录");

    let store = app.store("auth.json").map_err(|e| e.to_string())?;

    store.delete("credentials");
    store.save().map_err(|e| e.to_string())?;

    // 2. 清除内存中的 Token
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

// 4. 下载逻辑 (完全修复重定向问题)
#[tauri::command]
async fn download_file(
    file_id: i64,
    file_name: String,
    etag: String,
    s3_key_flag: String,
    size: i64,
    save_path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("开始下载文件: {} (ID: {})", file_name, file_id);

    let client = &state.client;
    let token = state.token.lock().unwrap().clone();

    // 步骤 1: 获取 API 返回的中间链接
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

    let res = req
        .send()
        .await
        .map_err(|e| format!("获取API错误: {}", e))?;
    let info_res: DownloadInfoResponse =
        res.json().await.map_err(|e| format!("解析失败: {}", e))?;

    if info_res.code != 0 {
        error!("下载鉴权失败: {}", info_res.message);
        return Err(format!("获取下载链接失败: {}", info_res.message));
    }

    let intermediate_url = info_res
        .data
        .map(|d| d.download_url)
        .ok_or("下载链接为空")?;

    // 步骤 2: 解析中间页 (核心修复！)
    // 创建一个一次性的、不跟随重定向的客户端
    let no_redirect_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none()) // 禁止自动跳转
        .user_agent("123pan/v2.4.0(Android_7.1.2;Xiaomi)")
        .build()
        .map_err(|e| format!("创建临时客户端失败: {}", e))?;

    let html_res = no_redirect_client
        .get(&intermediate_url)
        .send()
        .await
        .map_err(|e| format!("请求跳转页失败: {}", e))?;

    // 优先检查 Location 头（标准重定向），这比正则更稳、更快
    let final_download_url = if let Some(loc) = html_res.headers().get("location") {
        loc.to_str().unwrap_or_default().to_string()
    } else {
        // 如果没有 Location 头，说明返回的是 HTML 文本（如 Python 遇到的情况），则使用正则提取
        let html_text = html_res
            .text()
            .await
            .map_err(|e| format!("读取跳转页失败: {}", e))?;
        let re = Regex::new(r"href='(https?://[^']+)'").map_err(|e| format!("正则错误: {}", e))?;
        re.captures(&html_text)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
            .ok_or("解析真实下载地址失败")?
    };

    // 步骤 3: 真实下载
    let res = client
        .get(&final_download_url)
        .send()
        .await
        .map_err(|e| format!("文件请求失败: {}", e))?;

    let total_size = res.content_length().unwrap_or(size as u64);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            // 初始化日志插件，并配置轮转策略
            tauri_plugin_log::Builder::new()
                // 轮转策略: Keep(5) 表示只保留最近的5个日志文件
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                // 最大文件大小: 2MB (1024 * 1024 * 2 bytes)，超过后会自动切割
                .max_file_size(1024 * 1024 * 2)
                // 默认日志级别
                .level(log::LevelFilter::Info)
                .build(),
        )
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            login,
            get_file_list,
            download_file,
            try_auto_login,
            logout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
