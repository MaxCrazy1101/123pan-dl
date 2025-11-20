<script setup>
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save, open, confirm, message } from '@tauri-apps/plugin-dialog';

// --- 状态定义 ---
const username = ref("");
const password = ref("");
const isLoggedIn = ref(false);
const isLoading = ref(true);
const isFilesLoading = ref(false);

// 文件浏览相关
const fileList = ref([]);
const currentPathId = ref(0);
const pathHistory = ref([0]);

// 任务进度相关
const downloadStatus = ref({}); // { fileId: { progress, status } }
const uploadStatus = ref({});   // { filePath: { progress, status, name } }

// --- 生命周期 ---
let unlistenDownload;
let unlistenUpload;

onMounted(async () => {
    // 1. 监听下载进度
    unlistenDownload = await listen('download-progress', (event) => {
        const { id, progress, status } = event.payload;
        downloadStatus.value[id] = { progress, status };
        if (status === 'finished') {
            setTimeout(() => {
                if (downloadStatus.value[id]?.status === 'finished') {
                    delete downloadStatus.value[id];
                }
            }, 2000);
        }
    });

    // 2. 监听上传进度
    unlistenUpload = await listen('upload-progress', (event) => {
        const { id, progress, status } = event.payload;
        // 尝试从路径获取文件名用于显示
        const name = id.split(/[/\\]/).pop();
        uploadStatus.value[id] = { progress, status, name };

        if (status === 'finished') {
            setTimeout(() => {
                if (uploadStatus.value[id]?.status === 'finished') {
                    delete uploadStatus.value[id];
                    // 上传完成后自动刷新列表
                    if (Object.keys(uploadStatus.value).length === 0) {
                        refresh();
                    }
                }
            }, 2000);
        }
    });

    await checkAutoLogin();
});

onUnmounted(() => {
    if (unlistenDownload) unlistenDownload();
    if (unlistenUpload) unlistenUpload();
});

// --- 核心逻辑 ---

async function checkAutoLogin() {
    try {
        isLoading.value = true;
        const success = await invoke("try_auto_login");
        if (success) {
            isLoggedIn.value = true;
            await loadFiles(0);
        }
    } catch (error) {
        console.error("自动登录失败:", error);
    } finally {
        isLoading.value = false;
    }
}

async function handleLogin() {
    if (!username.value || !password.value) {
        await message("请输入用户名和密码", { title: "提示", kind: "warning" });
        return;
    }
    try {
        isLoading.value = true;
        const msg = await invoke("login", {
            username: username.value,
            password: password.value
        });
        isLoggedIn.value = true;
        await loadFiles(0);
    } catch (error) {
        await message("登录失败: " + error, { title: "错误", kind: "error" });
    } finally {
        isLoading.value = false;
    }
}

// 退出登录
async function handleLogout() {
    const yes = await confirm("确定要退出登录吗？", { title: '退出确认', kind: 'info' });
    if (!yes) return;

    try {
        await invoke("logout");
        isLoggedIn.value = false;
        fileList.value = [];
        currentPathId.value = 0;
        pathHistory.value = [0];
        username.value = "";
        password.value = "";
    } catch (e) {
        await message("退出失败: " + e, { title: "错误", kind: "error" });
    }
}

async function loadFiles(parentId) {
    try {
        isFilesLoading.value = true;
        const files = await invoke("get_file_list", { parentFileId: parentId });
        fileList.value = files;
        currentPathId.value = parentId;
    } catch (error) {
        await message("获取文件列表失败: " + error, { title: "错误", kind: "error" });
    } finally {
        isFilesLoading.value = false;
    }
}

// 导航操作
function enterFolder(folderId) {
    pathHistory.value.push(folderId);
    loadFiles(folderId);
}

function goBack() {
    if (pathHistory.value.length > 1) {
        pathHistory.value.pop();
        const parentId = pathHistory.value[pathHistory.value.length - 1];
        loadFiles(parentId);
    }
}

function refresh() {
    loadFiles(currentPathId.value);
}

function formatSize(size) {
    if (size > 1048576) return (size / 1048576).toFixed(2) + " MB";
    return (size / 1024).toFixed(2) + " KB";
}

// --- 功能操作 ---

// 上传文件
async function handleUpload() {
    try {
        // 打开文件选择对话框
        const selected = await open({
            multiple: false,
            directory: false,
        });

        if (!selected) return;

        const filePath = selected; // 选中文件的完整路径
        const fileName = filePath.split(/[/\\]/).pop();

        // 初始化状态
        uploadStatus.value[filePath] = { progress: 0, status: 'starting', name: fileName };

        // 调用后端
        await invoke("upload_file", {
            parentFileId: currentPathId.value,
            filePath: filePath
        });

    } catch (error) {
        await message("上传失败: " + error, { title: "错误", kind: "error" });
    }
}

// 新建文件夹
async function handleCreateFolder() {
    const name = prompt("请输入新文件夹名称:", "");
    if (!name) return;

    try {
        await invoke("create_folder", {
            parentFileId: currentPathId.value,
            folderName: name
        });
        refresh();
    } catch (error) {
        await message("创建文件夹失败: " + error, { title: "错误", kind: "error" });
    }
}

// 删除文件
async function handleDelete(file) {
    const yes = await confirm(
        `确定要删除 "${file.FileName}" 吗？\n注意：文件将移入回收站。`,
        {
            title: '删除确认',
            kind: 'warning'
        }
    );

    if (!yes) return;

    try {
        await invoke("delete_file", { fileId: file.FileId });
        refresh();
    } catch (error) {
        await message("删除失败: " + error, { title: "错误", kind: "error" });
    }
}

// 下载文件
async function handleDownload(file) {
    try {
        const savePath = await save({
            defaultPath: file.FileName,
        });

        if (!savePath) return;

        downloadStatus.value[file.FileId] = { progress: 0, status: 'starting' };

        await invoke("download_file", {
            fileId: file.FileId,
            fileName: file.FileName,
            fileType: file.Type,
            etag: file.Etag || "",
            s3KeyFlag: file.S3KeyFlag || "0",
            size: file.Size,
            savePath: savePath,
        });

    } catch (error) {
        await message("下载启动失败: " + error, { title: "错误", kind: "error" });
        delete downloadStatus.value[file.FileId];
    }
}

// 分享文件
async function handleShare(file) {
    const pwd = prompt("请输入提取码（可选，留空表示无需提取码）：", "");
    if (pwd === null) return;

    try {
        const result = await invoke("share_file", {
            fileIds: [file.FileId],
            sharePwd: pwd
        });

        let copyText = `链接: ${result.share_url}`;
        if (result.share_pwd) {
            copyText += ` 提取码: ${result.share_pwd}`;
        }

        try {
            await navigator.clipboard.writeText(copyText);
            await message("分享成功！链接已复制到剪贴板。\n\n" + copyText, { title: "成功", kind: "info" });
        } catch (err) {
            prompt("分享成功，请手动复制链接：", copyText);
        }

    } catch (error) {
        await message("分享失败: " + error, { title: "错误", kind: "error" });
    }
}
</script>

<template>
    <div class="app-container">
        <!-- 全局 Loading -->
        <div v-if="isLoading" class="loading-mask">
            <div class="spinner"></div>
            <p>正在连接 123Pan...</p>
        </div>

        <!-- 登录界面 -->
        <div v-else-if="!isLoggedIn" class="login-container">
            <div class="login-box">
                <h2>123Pan 客户端</h2>
                <input v-model="username" placeholder="手机号/用户名" />
                <input v-model="password" type="password" placeholder="密码" @keyup.enter="handleLogin" />
                <button @click="handleLogin" class="primary-btn">登录</button>
            </div>
        </div>

        <!-- 主界面 -->
        <div v-else class="main-interface">
            <!-- 顶部工具栏 -->
            <div class="toolbar">
                <div class="left-tools">
                    <button @click="goBack" :disabled="pathHistory.length <= 1" class="nav-btn">
                        ← 返回
                    </button>
                    <button @click="refresh" class="nav-btn">↻ 刷新</button>
                    <span class="path-info">ID: {{ currentPathId }}</span>
                </div>
                <div class="right-tools">
                    <button @click="handleUpload" class="nav-btn primary">⬆ 上传文件</button>
                    <button @click="handleCreateFolder" class="nav-btn">➕ 新建文件夹</button>
                    <button @click="handleLogout" class="nav-btn danger">退出登录</button>
                </div>
            </div>

            <!-- 文件列表 -->
            <div class="file-list-container">
                <div v-if="isFilesLoading" class="loading-files">加载中...</div>

                <table v-else>
                    <thead>
                        <tr>
                            <th style="width: 45%">文件名</th>
                            <th style="width: 15%">大小</th>
                            <th style="width: 10%">类型</th>
                            <th style="width: 30%">操作</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="file in fileList" :key="file.FileId"
                            @dblclick="file.Type === 1 ? enterFolder(file.FileId) : null">

                            <!-- 文件名列（包含下载进度条） -->
                            <td class="name-cell">
                                <div class="file-icon">{{ file.Type === 1 ? '📂' : '📄' }}</div>
                                <div class="file-info">
                                    <div class="file-name" :title="file.FileName">{{ file.FileName }}</div>

                                    <!-- 下载进度条组件 -->
                                    <div v-if="downloadStatus[file.FileId]" class="progress-wrapper">
                                        <div class="progress-track">
                                            <div class="progress-fill"
                                                :style="{ width: downloadStatus[file.FileId].progress + '%' }"
                                                :class="{ 'finished': downloadStatus[file.FileId].status === 'finished' }">
                                            </div>
                                        </div>
                                        <span class="progress-text">
                                            {{ downloadStatus[file.FileId].status === 'finished' ? '完成' :
                                                downloadStatus[file.FileId].progress + '%' }}
                                        </span>
                                    </div>
                                </div>
                            </td>

                            <td>{{ formatSize(file.Size) }}</td>
                            <td>{{ file.Type === 1 ? '文件夹' : '文件' }}</td>

                            <td>
                                <div class="action-buttons">
                                    <button v-if="file.Type === 0" @click="handleDownload(file)"
                                        class="action-btn download">下载</button>
                                    <button v-else @click="enterFolder(file.FileId)" class="action-btn open">打开</button>

                                    <button @click="handleShare(file)" class="action-btn share">分享</button>
                                    <button @click="handleDelete(file)" class="action-btn delete">删除</button>
                                </div>
                            </td>
                        </tr>
                        <tr v-if="fileList.length === 0">
                            <td colspan="4" style="text-align: center; padding: 40px; color: #888;">
                                此文件夹为空
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>

            <!-- 上传任务面板 -->
            <div v-if="Object.keys(uploadStatus).length > 0" class="upload-panel">
                <div class="panel-header">上传任务</div>
                <div class="panel-body">
                    <div v-for="(task, path) in uploadStatus" :key="path" class="upload-item">
                        <div class="upload-name" :title="task.name">{{ task.name }}</div>
                        <div class="progress-wrapper">
                            <div class="progress-track">
                                <div class="progress-fill" :style="{ width: task.progress + '%' }"
                                    :class="{ 'finished': task.status === 'finished' }"></div>
                            </div>
                            <span class="progress-text">
                                {{ task.status === 'hashing' ? '校验中' : (task.status === 'finished' ? '完成' :
                                    task.progress + '%') }}
                            </span>
                        </div>
                    </div>
                </div>
            </div>

        </div>
    </div>
</template>

<!-- 全局样式重置 (关键修复) -->
<style>
body,
html {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    /* 禁止外层滚动 */
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
}
</style>

<style scoped>
/* 基础布局 */
.app-container {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: #f5f7fa;
    color: #333;
    box-sizing: border-box;
    /* 确保容器内不溢出 */
    overflow: hidden;
}

/* Loading 遮罩 */
.loading-mask {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(255, 255, 255, 0.95);
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    z-index: 999;
}

.spinner {
    width: 40px;
    height: 40px;
    border: 4px solid #f3f3f3;
    border-top: 4px solid #409eff;
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin-bottom: 10px;
}

@keyframes spin {
    0% {
        transform: rotate(0deg);
    }

    100% {
        transform: rotate(360deg);
    }
}

/* 登录框 */
.login-container {
    display: flex;
    justify-content: center;
    align-items: center;
    height: 100%;
}

.login-box {
    background: white;
    padding: 40px;
    border-radius: 12px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.05);
    width: 320px;
    display: flex;
    flex-direction: column;
    gap: 20px;
}

.login-box h2 {
    margin: 0 0 10px 0;
    text-align: center;
    color: #409eff;
}

.login-box input {
    padding: 12px;
    border: 1px solid #dcdfe6;
    border-radius: 6px;
    outline: none;
    transition: border-color 0.2s;
}

.login-box input:focus {
    border-color: #409eff;
}

/* 按钮通用样式 */
button {
    cursor: pointer;
    font-size: 14px;
    transition: all 0.2s;
}

.primary-btn {
    background-color: #409eff;
    color: white;
    padding: 12px;
    border: none;
    border-radius: 6px;
    font-weight: 600;
}

.primary-btn:hover {
    background-color: #66b1ff;
}

.nav-btn {
    padding: 6px 16px;
    background: white;
    border: 1px solid #dcdfe6;
    border-radius: 6px;
    margin-left: 10px;
    color: #606266;
}

.nav-btn:hover {
    color: #409eff;
    border-color: #c6e2ff;
    background-color: #ecf5ff;
}

.nav-btn.primary {
    background-color: #409eff;
    color: white;
    border-color: #409eff;
}

.nav-btn.primary:hover {
    background-color: #66b1ff;
    border-color: #66b1ff;
}

.nav-btn.danger {
    color: #f56c6c;
    border-color: #fbc4c4;
    background-color: #fef0f0;
}

.nav-btn.danger:hover {
    background-color: #f56c6c;
    color: white;
    border-color: #f56c6c;
}

.nav-btn:disabled {
    cursor: not-allowed;
    opacity: 0.6;
}

/* 主界面 */
.main-interface {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
}

.toolbar {
    padding: 12px 20px;
    background: white;
    border-bottom: 1px solid #ebeef5;
    display: flex;
    justify-content: space-between;
    align-items: center;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.02);
    /* 防止工具栏被压缩 */
    flex-shrink: 0;
}

.path-info {
    color: #909399;
    font-size: 13px;
    margin-left: 15px;
}

/* 文件列表 */
.file-list-container {
    flex: 1;
    overflow-y: auto;
    /* 只在这里显示垂直滚动条 */
    overflow-x: hidden;
    padding: 20px;
    box-sizing: border-box;
}

.loading-files {
    text-align: center;
    color: #909399;
    margin-top: 50px;
}

table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    background: white;
    border-radius: 8px;
    overflow: hidden;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.05);
}

th {
    background: #f5f7fa;
    padding: 15px;
    text-align: left;
    font-weight: 600;
    color: #606266;
    border-bottom: 1px solid #ebeef5;
    position: sticky;
    top: 0;
    /* 表头固定 */
    z-index: 10;
}

td {
    padding: 15px;
    border-bottom: 1px solid #ebeef5;
    vertical-align: middle;
}

tr:last-child td {
    border-bottom: none;
}

tr:hover {
    background-color: #fdfdfd;
}

/* 单元格 */
.name-cell {
    display: flex;
    align-items: center;
}

.file-icon {
    font-size: 24px;
    margin-right: 12px;
}

.file-info {
    flex: 1;
    min-width: 0;
}

.file-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
}

/* 操作按钮组 */
.action-buttons {
    display: flex;
    gap: 8px;
}

.action-btn {
    padding: 5px 10px;
    border: 1px solid #dcdfe6;
    border-radius: 4px;
    font-size: 12px;
    background: white;
    color: #606266;
}

.action-btn:hover {
    border-color: #409eff;
    color: #409eff;
}

.action-btn.download {
    background: #f0f9eb;
    border-color: #e1f3d8;
    color: #67c23a;
}

.action-btn.download:hover {
    background: #67c23a;
    color: white;
    border-color: #67c23a;
}

.action-btn.share {
    background: #fdf6ec;
    border-color: #faecd8;
    color: #e6a23c;
}

.action-btn.share:hover {
    background: #e6a23c;
    color: white;
    border-color: #e6a23c;
}

.action-btn.delete {
    background: #fef0f0;
    border-color: #fde2e2;
    color: #f56c6c;
}

.action-btn.delete:hover {
    background: #f56c6c;
    color: white;
    border-color: #f56c6c;
}

/* 进度条通用样式 */
.progress-wrapper {
    margin-top: 6px;
    display: flex;
    align-items: center;
    gap: 8px;
}

.progress-track {
    flex: 1;
    height: 6px;
    background: #ebeef5;
    border-radius: 3px;
    overflow: hidden;
}

.progress-fill {
    height: 100%;
    background: #409eff;
    transition: width 0.3s ease;
}

.progress-fill.finished {
    background: #67c23a;
}

.progress-text {
    font-size: 11px;
    color: #909399;
    min-width: 35px;
    text-align: right;
}

/* 上传面板 */
.upload-panel {
    position: fixed;
    bottom: 20px;
    right: 20px;
    width: 320px;
    background: white;
    border-radius: 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
    overflow: hidden;
    border: 1px solid #ebeef5;
    z-index: 100;
}

.panel-header {
    padding: 10px 15px;
    background: #409eff;
    color: white;
    font-weight: 600;
    font-size: 14px;
}

.panel-body {
    max-height: 300px;
    overflow-y: auto;
    padding: 10px;
}

.upload-item {
    margin-bottom: 12px;
    border-bottom: 1px solid #f5f5f5;
    padding-bottom: 8px;
}

.upload-item:last-child {
    margin-bottom: 0;
    border-bottom: none;
    padding-bottom: 0;
}

.upload-name {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 4px;
}
</style>