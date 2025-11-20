<script setup>
import { ref, onMounted, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from '@tauri-apps/plugin-dialog';

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

// 下载进度相关
const downloadStatus = ref({});

// --- 生命周期 ---
let unlisten;

onMounted(async () => {
    unlisten = await listen('download-progress', (event) => {
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

    await checkAutoLogin();
});

onUnmounted(() => {
    if (unlisten) unlisten();
});

// --- 核心逻辑 ---

// 自动登录检查
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

// 手动登录
async function handleLogin() {
    if (!username.value || !password.value) {
        alert("请输入用户名和密码");
        return;
    }
    try {
        isLoading.value = true;
        const msg = await invoke("login", {
            username: username.value,
            password: password.value
        });
        alert(msg); // 可以考虑把这个弹窗去掉，直接进入
        isLoggedIn.value = true;
        await loadFiles(0);
    } catch (error) {
        alert("登录失败: " + error);
    } finally {
        isLoading.value = false;
    }
}

// --- 新增：退出登录逻辑 ---
async function handleLogout() {
    if (!confirm("确定要退出登录吗？")) return;

    try {
        await invoke("logout");
        // 重置状态
        isLoggedIn.value = false;
        fileList.value = [];
        currentPathId.value = 0;
        pathHistory.value = [0];
        username.value = "";
        password.value = "";
    } catch (e) {
        alert("退出失败: " + e);
    }
}

// 加载文件列表
async function loadFiles(parentId) {
    try {
        isFilesLoading.value = true;
        const files = await invoke("get_file_list", { parentFileId: parentId });
        fileList.value = files;
        currentPathId.value = parentId;
    } catch (error) {
        alert("获取文件列表失败: " + error);
    } finally {
        isFilesLoading.value = false;
    }
}

// 进入文件夹
function enterFolder(folderId) {
    pathHistory.value.push(folderId);
    loadFiles(folderId);
}

// 返回上一级
function goBack() {
    if (pathHistory.value.length > 1) {
        pathHistory.value.pop();
        const parentId = pathHistory.value[pathHistory.value.length - 1];
        loadFiles(parentId);
    }
}

// 刷新当前目录
function refresh() {
    loadFiles(currentPathId.value);
}

// 格式化文件大小
function formatSize(size) {
    if (size > 1048576) return (size / 1048576).toFixed(2) + " MB";
    return (size / 1024).toFixed(2) + " KB";
}

// 处理下载
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
            etag: file.Etag || "",
            s3KeyFlag: file.S3KeyFlag || "0", // 传递 S3KeyFlag
            size: file.Size,
            savePath: savePath,
        });

    } catch (error) {
        alert("下载启动失败: " + error);
        delete downloadStatus.value[file.FileId];
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
                    <!-- 新增退出按钮 -->
                    <button @click="handleLogout" class="nav-btn danger">退出登录</button>
                </div>
            </div>

            <!-- 文件列表 -->
            <div class="file-list-container">
                <div v-if="isFilesLoading" class="loading-files">加载中...</div>

                <table v-else>
                    <thead>
                        <tr>
                            <th style="width: 50%">文件名</th>
                            <th style="width: 20%">大小</th>
                            <th style="width: 15%">类型</th>
                            <th style="width: 15%">操作</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="file in fileList" :key="file.FileId"
                            @dblclick="file.Type === 1 ? enterFolder(file.FileId) : null">

                            <!-- 文件名列（包含进度条） -->
                            <td class="name-cell">
                                <div class="file-icon">{{ file.Type === 1 ? '📂' : '📄' }}</div>
                                <div class="file-info">
                                    <div class="file-name">{{ file.FileName }}</div>

                                    <!-- 进度条组件 -->
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
                                <button v-if="file.Type === 0" @click="handleDownload(file)"
                                    class="action-btn download">下载</button>
                                <button v-else @click="enterFolder(file.FileId)" class="action-btn open">打开</button>
                            </td>
                        </tr>
                        <tr v-if="fileList.length === 0">
                            <td colspan="4" style="text-align: center; padding: 20px; color: #888;">
                                空文件夹
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
    </div>
</template>

<style scoped>
/* 基础布局 */
.app-container {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: #f5f7fa;
    color: #333;
    font-family: sans-serif;
}

/* Loading 遮罩 */
.loading-mask {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(255, 255, 255, 0.9);
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
    border-top: 4px solid #3498db;
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
    padding: 30px;
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
    width: 300px;
    display: flex;
    flex-direction: column;
    gap: 15px;
}

.login-box input {
    padding: 10px;
    border: 1px solid #ddd;
    border-radius: 4px;
}

/* 按钮样式 */
.primary-btn {
    background-color: #409eff;
    color: white;
    padding: 10px;
    border: none;
    border-radius: 4px;
    cursor: pointer;
}

.primary-btn:hover {
    background-color: #66b1ff;
}

.nav-btn {
    padding: 6px 12px;
    background: white;
    border: 1px solid #dcdfe6;
    border-radius: 4px;
    cursor: pointer;
    margin-right: 10px;
    transition: all 0.2s;
}

.nav-btn:hover {
    background-color: #f2f6fc;
    border-color: #c6e2ff;
    color: #409eff;
}

.nav-btn:disabled {
    cursor: not-allowed;
    opacity: 0.5;
}

/* 退出按钮样式 */
.nav-btn.danger {
    color: #f56c6c;
    border-color: #fde2e2;
    background-color: #fef0f0;
}

.nav-btn.danger:hover {
    background-color: #f56c6c;
    color: white;
    border-color: #f56c6c;
}

.action-btn {
    padding: 4px 10px;
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
}

.action-btn.download {
    background: #e1f3d8;
    color: #67c23a;
}

.action-btn.open {
    background: #ecf5ff;
    color: #409eff;
}

/* 主界面 */
.main-interface {
    display: flex;
    flex-direction: column;
    height: 100%;
}

.toolbar {
    padding: 10px 20px;
    background: white;
    border-bottom: 1px solid #eee;
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.path-info {
    color: #909399;
    font-size: 12px;
    margin-left: 10px;
}

/* 表格样式 */
.file-list-container {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
}

table {
    width: 100%;
    border-collapse: collapse;
    background: white;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.05);
}

th {
    background: #fafafa;
    padding: 12px;
    text-align: left;
    font-weight: 600;
    color: #606266;
    border-bottom: 1px solid #ebeef5;
}

td {
    padding: 12px;
    border-bottom: 1px solid #ebeef5;
    vertical-align: middle;
}

tr:hover {
    background-color: #f5f7fa;
}

/* 文件名单元格布局 */
.name-cell {
    display: flex;
    align-items: center;
}

.file-icon {
    font-size: 20px;
    margin-right: 10px;
}

.file-info {
    flex: 1;
}

/* 进度条样式 */
.progress-wrapper {
    margin-top: 4px;
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
    font-size: 10px;
    color: #909399;
    min-width: 30px;
}
</style>