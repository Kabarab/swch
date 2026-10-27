#pragma once

#include <windows.h>
#include <string>

namespace swch_single_instance {

constexpr const wchar_t* kDefaultMutexName = L"Local\\SWCH_Application_SingleInstance_Mutex";
constexpr const wchar_t* kWindowClassName = L"SWCH_MAIN_WINDOW";

/// RAII wrapper for named mutex ensuring only one application instance runs per user session.
class InstanceLock {
public:
    explicit InstanceLock(const wchar_t* mutexName = kDefaultMutexName) {
        m_hMutex = CreateMutexW(nullptr, TRUE, mutexName);
        const DWORD error = GetLastError();
        m_isFirst = (m_hMutex != nullptr && error != ERROR_ALREADY_EXISTS);
    }

    ~InstanceLock() {
        Release();
    }

    InstanceLock(const InstanceLock&) = delete;
    InstanceLock& operator=(const InstanceLock&) = delete;

    bool IsFirstInstance() const {
        return m_isFirst;
    }

    void Release() {
        if (m_hMutex != nullptr) {
            if (m_isFirst) {
                ReleaseMutex(m_hMutex);
            }
            CloseHandle(m_hMutex);
            m_hMutex = nullptr;
            m_isFirst = false;
        }
    }

private:
    HANDLE m_hMutex = nullptr;
    bool m_isFirst = false;
};

/// Finds the existing SWCH main window by class name.
inline HWND FindExistingWindow(const wchar_t* className = kWindowClassName) {
    return FindWindowW(className, nullptr);
}

/// Polls for the existing window in case the first instance is still finishing window creation.
inline HWND WaitForExistingWindow(const wchar_t* className = kWindowClassName, int maxRetries = 20, DWORD sleepMs = 50) {
    for (int i = 0; i < maxRetries; ++i) {
        HWND hwnd = FindExistingWindow(className);
        if (hwnd) return hwnd;
        Sleep(sleepMs);
    }
    return nullptr;
}

/// Restores and activates the existing application window in the foreground.
inline bool ActivateExistingWindow(HWND hwnd) {
    if (!hwnd || !IsWindow(hwnd)) return false;

    DWORD targetPid = 0;
    DWORD targetThread = GetWindowThreadProcessId(hwnd, &targetPid);
    DWORD currentThread = GetCurrentThreadId();

    if (targetPid != 0) {
        AllowSetForegroundWindow(targetPid);
    }

    if (IsIconic(hwnd)) {
        ShowWindow(hwnd, SW_RESTORE);
    } else {
        ShowWindow(hwnd, SW_SHOW);
    }

    if (targetThread != 0 && targetThread != currentThread) {
        AttachThreadInput(currentThread, targetThread, TRUE);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        AttachThreadInput(currentThread, targetThread, FALSE);
    } else {
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
    }

    BringWindowToTop(hwnd);
    return true;
}

} // namespace swch_single_instance
