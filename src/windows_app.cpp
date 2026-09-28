#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <shlwapi.h>
#include <winhttp.h>
#include <dwmapi.h>
#include <wrl/client.h>
#include <functional>
#include <string>
#include "webview2/WebView2.h"
#include "embedded_loader.h"

#ifndef DWMWA_USE_IMMERSIVE_DARK_MODE
#define DWMWA_USE_IMMERSIVE_DARK_MODE 20
#endif

#define TIMER_HEALTH_ID 1001
#define MAX_HEALTH_RETRIES 50 // 50 * 200ms = 10 seconds

using Microsoft::WRL::ComPtr;

// Custom COM callback template to avoid dependency on MSVC wrl/event.h
template <typename TInterface, typename... Args>
class CallbackImpl : public TInterface {
    LONG m_refCount;
    std::function<HRESULT(Args...)> m_func;
public:
    CallbackImpl(std::function<HRESULT(Args...)> func) : m_refCount(1), m_func(func) {}

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppvObject) override {
        if (!ppvObject) return E_POINTER;
        *ppvObject = static_cast<TInterface*>(this);
        AddRef();
        return S_OK;
    }

    ULONG STDMETHODCALLTYPE AddRef() override {
        return InterlockedIncrement(&m_refCount);
    }

    ULONG STDMETHODCALLTYPE Release() override {
        LONG count = InterlockedDecrement(&m_refCount);
        if (count == 0) {
            delete this;
            return 0;
        }
        return count;
    }

    HRESULT STDMETHODCALLTYPE Invoke(Args... args) override {
        return m_func(args...);
    }
};

typedef HRESULT (STDAPICALLTYPE *CreateCoreWebView2EnvironmentWithOptionsFn)(
    PCWSTR browserExecutableFolder,
    PCWSTR userDataFolder,
    ICoreWebView2EnvironmentOptions* environmentOptions,
    ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler* environment_created_handler);

// Global application state
static HWND g_hWnd = NULL;
static ComPtr<ICoreWebView2Controller> g_controller;
static ComPtr<ICoreWebView2> g_webview;
static HANDLE g_hServerProcess = NULL;
static int g_retryCount = 0;
static bool g_backendReady = false;

static const wchar_t* kSplashHtml = LR"rawhtml(
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    body { margin: 0; background: #111214; color: #f2f2f4; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: grid; min-height: 100vh; place-items: center; user-select: none; }
    .card { text-align: center; }
    .spinner { width: 36px; height: 36px; border: 3px solid rgba(255,100,45,0.2); border-top-color: #ff642d; border-radius: 50%; animation: spin 0.8s linear infinite; margin: 0 auto 16px; }
    @keyframes spin { to { transform: rotate(360deg); } }
    h2 { margin: 0 0 8px; font-size: 20px; font-weight: 600; }
    p { margin: 0; color: #888; font-size: 13px; }
  </style>
</head>
<body>
  <div class="card">
    <div class="spinner"></div>
    <h2>Запуск SWCH...</h2>
    <p>Инициализация игрового пространства</p>
  </div>
</body>
</html>
)rawhtml";

static const wchar_t* kErrorHtml = LR"rawhtml(
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    body { margin: 0; background: #111214; color: #f2f2f4; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: grid; min-height: 100vh; place-items: center; }
    .box { max-width: 440px; padding: 32px; border: 1px solid #36373d; border-radius: 16px; background: #1b1c20; text-align: center; }
    h2 { margin: 0 0 12px; font-size: 22px; }
    p { margin: 0 0 20px; color: #adb0bb; font-size: 14px; line-height: 1.5; }
    button { background: #ff642d; border: none; color: #111; font-weight: 700; padding: 10px 22px; border-radius: 8px; cursor: pointer; font-size: 14px; }
  </style>
</head>
<body>
  <div class="box">
    <h2>Не удалось запустить SWCH</h2>
    <p>Серверный движок не ответил вовремя. Проверьте запуск приложения или повторите попытку.</p>
    <button onclick="window.chrome.webview.postMessage('retry')">Повторить попытку</button>
  </div>
</body>
</html>
)rawhtml";

// Check health of Axum backend at http://127.0.0.1:4242/api/health
bool CheckBackendHealth() {
    HINTERNET hSession = WinHttpOpen(L"swch-launcher/1.2.0",
                                     WINHTTP_ACCESS_TYPE_NO_PROXY,
                                     WINHTTP_NO_PROXY_NAME,
                                     WINHTTP_NO_PROXY_BYPASS, 0);
    if (!hSession) return false;

    WinHttpSetTimeouts(hSession, 400, 400, 400, 400);

    HINTERNET hConnect = WinHttpConnect(hSession, L"127.0.0.1", 4242, 0);
    if (!hConnect) {
        WinHttpCloseHandle(hSession);
        return false;
    }

    HINTERNET hRequest = WinHttpOpenRequest(hConnect, L"GET", L"/api/health",
                                            NULL, WINHTTP_NO_REFERER,
                                            WINHTTP_DEFAULT_ACCEPT_TYPES, 0);
    if (!hRequest) {
        WinHttpCloseHandle(hConnect);
        WinHttpCloseHandle(hSession);
        return false;
    }

    BOOL bResults = WinHttpSendRequest(hRequest, WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                                       WINHTTP_NO_REQUEST_DATA, 0, 0, 0);
    if (bResults) {
        bResults = WinHttpReceiveResponse(hRequest, NULL);
    }

    bool healthy = false;
    if (bResults) {
        DWORD dwStatusCode = 0;
        DWORD dwSize = sizeof(dwStatusCode);
        if (WinHttpQueryHeaders(hRequest, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                                WINHTTP_HEADER_NAME_BY_INDEX, &dwStatusCode, &dwSize, WINHTTP_NO_HEADER_INDEX)) {
            if (dwStatusCode == 200) {
                healthy = true;
            }
        }
    }

    WinHttpCloseHandle(hRequest);
    WinHttpCloseHandle(hConnect);
    WinHttpCloseHandle(hSession);
    return healthy;
}

// Background thread monitoring backend process; exits GUI shell if backend terminates
DWORD WINAPI ServerWatcherThread(LPVOID lpParam) {
    HWND hwnd = (HWND)lpParam;
    if (g_hServerProcess != NULL) {
        WaitForSingleObject(g_hServerProcess, INFINITE);
        // If server process terminated (e.g. self-update restart or crash), close GUI shell
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
    return 0;
}

// Launches swch-server.exe in the background with hidden window and assigns to Job Object
void LaunchServerProcess() {
    if (g_hServerProcess != NULL) return;

    // Check if backend is already running (e.g. external run or previous start)
    if (CheckBackendHealth()) {
        g_backendReady = true;
        return;
    }

    WCHAR exeDir[MAX_PATH];
    GetModuleFileNameW(NULL, exeDir, MAX_PATH);
    PathRemoveFileSpecW(exeDir);

    // Look for backend server binary
    WCHAR serverPath[MAX_PATH];
    const WCHAR* candidateNames[] = {
        L"swch-server.exe",
        L"swch_server.exe",
        L"swch_backend.exe"
    };

    bool found = false;
    for (const auto& name : candidateNames) {
        PathCombineW(serverPath, exeDir, name);
        if (GetFileAttributesW(serverPath) != INVALID_FILE_ATTRIBUTES) {
            found = true;
            break;
        }
    }

    if (!found) {
        // Fallback: check if swch.exe exists in same directory
        PathCombineW(serverPath, exeDir, L"swch.exe");
        if (GetFileAttributesW(serverPath) == INVALID_FILE_ATTRIBUTES) {
            return;
        }
    }

    STARTUPINFOW si = { sizeof(si) };
    PROCESS_INFORMATION pi = { 0 };

    WCHAR cmdLine[MAX_PATH * 2];
    wsprintfW(cmdLine, L"\"%s\"", serverPath);

    // CREATE_NO_WINDOW ensures no console window is created for the background server
    BOOL success = CreateProcessW(
        serverPath,
        cmdLine,
        NULL,
        NULL,
        FALSE,
        CREATE_NO_WINDOW,
        NULL,
        exeDir,
        &si,
        &pi
    );

    if (success) {
        g_hServerProcess = pi.hProcess;
        CloseHandle(pi.hThread);

        // Bind child process to Job Object with KILL_ON_JOB_CLOSE
        // Guarantee: If swch.exe terminates (normal, crash, task manager kill),
        // the Windows kernel automatically terminates swch-server.exe!
        HANDLE hJob = CreateJobObjectW(NULL, NULL);
        if (hJob) {
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION jeli = { 0 };
            jeli.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(hJob, JobObjectExtendedLimitInformation, &jeli, sizeof(jeli));
            AssignProcessToJobObject(hJob, pi.hProcess);
        }

        CreateThread(NULL, 0, ServerWatcherThread, (LPVOID)g_hWnd, 0, NULL);
    }
}

void StartBackendAndPolling() {
    g_retryCount = 0;
    if (g_webview) {
        g_webview->NavigateToString(kSplashHtml);
    }

    LaunchServerProcess();

    if (CheckBackendHealth()) {
        g_backendReady = true;
        if (g_webview) {
            g_webview->Navigate(L"http://127.0.0.1:4242");
        }
    } else {
        SetTimer(g_hWnd, TIMER_HEALTH_ID, 200, NULL);
    }
}

// Window Procedure
LRESULT CALLBACK WndProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
    case WM_SIZE:
        if (g_controller != nullptr) {
            RECT bounds;
            GetClientRect(hwnd, &bounds);
            g_controller->put_Bounds(bounds);
        }
        return 0;

    case WM_TIMER:
        if (wParam == TIMER_HEALTH_ID) {
            if (CheckBackendHealth()) {
                KillTimer(hwnd, TIMER_HEALTH_ID);
                g_backendReady = true;
                if (g_webview) {
                    g_webview->Navigate(L"http://127.0.0.1:4242");
                }
            } else {
                g_retryCount++;
                if (g_retryCount >= MAX_HEALTH_RETRIES) {
                    KillTimer(hwnd, TIMER_HEALTH_ID);
                    if (g_webview) {
                        g_webview->NavigateToString(kErrorHtml);
                    }
                }
            }
            return 0;
        }
        break;

    case WM_CLOSE:
        DestroyWindow(hwnd);
        return 0;

    case WM_DESTROY:
        if (g_hServerProcess != NULL) {
            TerminateProcess(g_hServerProcess, 0);
            CloseHandle(g_hServerProcess);
            g_hServerProcess = NULL;
        }
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, uMsg, wParam, lParam);
}

// WinMain entry point with subsystem: WINDOWS (no console window)
int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, PWSTR pCmdLine, int nCmdShow) {
    // Initialize COM
    HRESULT hr = CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) return 1;

    // Register Window Class
    const wchar_t CLASS_NAME[] = L"SWCH_MAIN_WINDOW";
    HBRUSH hDarkBrush = CreateSolidBrush(RGB(17, 18, 20));

    WNDCLASSEXW wc = { 0 };
    wc.cbSize = sizeof(wc);
    wc.style = CS_HREDRAW | CS_VREDRAW;
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hInstance;
    wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = hDarkBrush;
    wc.lpszClassName = CLASS_NAME;

    RegisterClassExW(&wc);

    // Center window on primary display (1280x820)
    int width = 1280;
    int height = 820;
    int screenW = GetSystemMetrics(SM_CXSCREEN);
    int screenH = GetSystemMetrics(SM_CYSCREEN);
    int posX = (screenW - width) / 2;
    int posY = (screenH - height) / 2;
    if (posX < 0) posX = CW_USEDEFAULT;
    if (posY < 0) posY = CW_USEDEFAULT;

    HWND hwnd = CreateWindowExW(
        0,
        CLASS_NAME,
        L"swch Hub",
        WS_OVERLAPPEDWINDOW,
        posX, posY, width, height,
        NULL,
        NULL,
        hInstance,
        NULL
    );

    if (!hwnd) {
        CoUninitialize();
        return 1;
    }

    g_hWnd = hwnd;

    // Apply immersive dark mode for window titlebar
    BOOL darkMode = TRUE;
    DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &darkMode, sizeof(darkMode));

    ShowWindow(hwnd, nCmdShow);
    UpdateWindow(hwnd);

    // Locate WebView2Loader.dll
    WCHAR exeDir[MAX_PATH];
    GetModuleFileNameW(NULL, exeDir, MAX_PATH);
    PathRemoveFileSpecW(exeDir);

    WCHAR loaderPath[MAX_PATH];
    PathCombineW(loaderPath, exeDir, L"WebView2Loader.dll");

    HMODULE hLoader = LoadLibraryW(loaderPath);

    // If not found in application directory, extract/load from %LOCALAPPDATA%\swch\WebView2Loader.dll
    if (!hLoader) {
        WCHAR localAppData[MAX_PATH];
        if (GetEnvironmentVariableW(L"LOCALAPPDATA", localAppData, MAX_PATH) > 0) {
            WCHAR appDataDir[MAX_PATH];
            PathCombineW(appDataDir, localAppData, L"swch");
            CreateDirectoryW(appDataDir, NULL);
            WCHAR appDataLoaderPath[MAX_PATH];
            PathCombineW(appDataLoaderPath, appDataDir, L"WebView2Loader.dll");

            if (GetFileAttributesW(appDataLoaderPath) == INVALID_FILE_ATTRIBUTES) {
                HANDLE hFile = CreateFileW(appDataLoaderPath, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
                if (hFile != INVALID_HANDLE_VALUE) {
                    DWORD bytesWritten = 0;
                    WriteFile(hFile, kEmbeddedWebView2Loader, sizeof(kEmbeddedWebView2Loader), &bytesWritten, NULL);
                    CloseHandle(hFile);
                }
            }
            hLoader = LoadLibraryW(appDataLoaderPath);
        }
    }

    if (!hLoader) {
        hLoader = LoadLibraryW(L"WebView2Loader.dll");
    }

    if (!hLoader) {
        int choice = MessageBoxW(
            hwnd,
            L"Не удалось загрузить библиотеку WebView2Loader.dll.\n\nДля работы интерфейса SWCH требуется Microsoft Edge WebView2.\nОткрыть официальную страницу загрузки WebView2 Runtime?",
            L"SWCH — Требуется WebView2 Runtime",
            MB_YESNO | MB_ICONEXCLAMATION
        );
        if (choice == IDYES) {
            ShellExecuteW(NULL, L"open", L"https://go.microsoft.com/fwlink/p/?LinkId=2124703", NULL, NULL, SW_SHOWNORMAL);
        }
        DestroyWindow(hwnd);
        CoUninitialize();
        return 1;
    }

    auto fnCreateEnv = (CreateCoreWebView2EnvironmentWithOptionsFn)GetProcAddress(
        hLoader, "CreateCoreWebView2EnvironmentWithOptions"
    );

    if (!fnCreateEnv) {
        MessageBoxW(hwnd, L"Неверная версия WebView2Loader.dll", L"Ошибка SWCH", MB_OK | MB_ICONERROR);
        DestroyWindow(hwnd);
        CoUninitialize();
        return 1;
    }

    // Set user data directory in %LOCALAPPDATA%\swch\webview2_data
    WCHAR userDataFolder[MAX_PATH];
    WCHAR localAppData[MAX_PATH];
    if (GetEnvironmentVariableW(L"LOCALAPPDATA", localAppData, MAX_PATH) > 0) {
        PathCombineW(userDataFolder, localAppData, L"swch");
        CreateDirectoryW(userDataFolder, NULL);
        PathCombineW(userDataFolder, userDataFolder, L"webview2_data");
        CreateDirectoryW(userDataFolder, NULL);
    } else {
        GetTempPathW(MAX_PATH, userDataFolder);
        PathAppendW(userDataFolder, L"swch_wv2");
        CreateDirectoryW(userDataFolder, NULL);
    }

    // Initialize WebView2 Environment
    HRESULT envHr = fnCreateEnv(
        nullptr,
        userDataFolder,
        nullptr,
        new CallbackImpl<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, HRESULT, ICoreWebView2Environment*>(
            [hwnd](HRESULT errorCode, ICoreWebView2Environment* env) -> HRESULT {
                if (FAILED(errorCode) || !env) {
                    int choice = MessageBoxW(
                        hwnd,
                        L"Не удалось инициализировать Microsoft Edge WebView2 Runtime.\n\nЗагрузить и установить официальный WebView2 Runtime с сайта Microsoft?",
                        L"SWCH — Ошибка инициализации",
                        MB_YESNO | MB_ICONINFORMATION
                    );
                    if (choice == IDYES) {
                        ShellExecuteW(NULL, L"open", L"https://go.microsoft.com/fwlink/p/?LinkId=2124703", NULL, NULL, SW_SHOWNORMAL);
                    }
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    return errorCode;
                }

                // Create CoreWebView2Controller
                env->CreateCoreWebView2Controller(
                    hwnd,
                    new CallbackImpl<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler, HRESULT, ICoreWebView2Controller*>(
                        [hwnd](HRESULT cErr, ICoreWebView2Controller* controller) -> HRESULT {
                            if (FAILED(cErr) || !controller) {
                                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                                return cErr;
                            }

                            g_controller = controller;
                            g_controller->get_CoreWebView2(&g_webview);

                            RECT bounds;
                            GetClientRect(hwnd, &bounds);
                            g_controller->put_Bounds(bounds);
                            g_controller->put_IsVisible(TRUE);

                            // Handle WebMessage from webview (e.g. retry button in error screen)
                            EventRegistrationToken token;
                            g_webview->add_WebMessageReceived(
                                new CallbackImpl<ICoreWebView2WebMessageReceivedEventHandler, ICoreWebView2*, ICoreWebView2WebMessageReceivedEventArgs*>(
                                    [](ICoreWebView2* sender, ICoreWebView2WebMessageReceivedEventArgs* args) -> HRESULT {
                                        LPWSTR msg = nullptr;
                                        if (SUCCEEDED(args->TryGetWebMessageAsString(&msg)) && msg != nullptr) {
                                            if (wcscmp(msg, L"retry") == 0) {
                                                StartBackendAndPolling();
                                            }
                                            CoTaskMemFree(msg);
                                        }
                                        return S_OK;
                                    }
                                ),
                                &token
                            );

                            // Start backend and begin polling
                            StartBackendAndPolling();
                            return S_OK;
                        }
                    )
                );
                return S_OK;
            }
        )
    );

    if (FAILED(envHr)) {
        MessageBoxW(hwnd, L"Ошибка вызова CreateCoreWebView2EnvironmentWithOptions", L"Ошибка SWCH", MB_OK | MB_ICONERROR);
        DestroyWindow(hwnd);
        CoUninitialize();
        return 1;
    }

    // Windows Message Loop
    MSG msg;
    while (GetMessageW(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    DeleteObject(hDarkBrush);
    CoUninitialize();
    return (int)msg.wParam;
}
