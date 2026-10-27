#include "../src/windows_single_instance.h"
#include <cassert>
#include <iostream>

static LRESULT CALLBACK TestWndProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    return DefWindowProcW(hwnd, uMsg, wParam, lParam);
}

int main() {
    const std::wstring testMutexName = L"Local\\SWCH_Test_Mutex_" + std::to_wstring(GetCurrentProcessId());

    // 1. First instance acquires lock
    {
        swch_single_instance::InstanceLock lock1(testMutexName.c_str());
        assert(lock1.IsFirstInstance() && "First instance lock must succeed");

        // 2. Second instance attempting the same mutex must detect an existing instance
        swch_single_instance::InstanceLock lock2(testMutexName.c_str());
        assert(!lock2.IsFirstInstance() && "Second instance must detect running instance");
    }

    // 3. After lock1 is destroyed, a new lock can be acquired
    {
        swch_single_instance::InstanceLock lock3(testMutexName.c_str());
        assert(lock3.IsFirstInstance() && "Lock should be re-acquirable after release");

        // Explicit Release() should also free the mutex
        lock3.Release();
        assert(!lock3.IsFirstInstance() && "Released lock should report not first instance");

        swch_single_instance::InstanceLock lock4(testMutexName.c_str());
        assert(lock4.IsFirstInstance() && "Lock should be acquirable after explicit Release()");
    }

    // 4. Non-existent window lookup times out and returns nullptr
    {
        const std::wstring nonexistentClass = L"SWCH_NonExistent_Class_" + std::to_wstring(GetCurrentProcessId());
        HWND missing = swch_single_instance::WaitForExistingWindow(nonexistentClass.c_str(), 2, 10);
        assert(missing == nullptr && "Missing window should return nullptr");
    }

    // 5. Window creation, detection, and activation (restoring from minimized)
    {
        const std::wstring testClassName = L"SWCH_TEST_WINDOW_" + std::to_wstring(GetCurrentProcessId());
        HINSTANCE hInstance = GetModuleHandleW(nullptr);

        WNDCLASSEXW wc = {};
        wc.cbSize = sizeof(wc);
        wc.lpfnWndProc = TestWndProc;
        wc.hInstance = hInstance;
        wc.lpszClassName = testClassName.c_str();
        ATOM atom = RegisterClassExW(&wc);
        assert(atom != 0 && "Test window class registration must succeed");

        HWND hwnd = CreateWindowExW(
            0,
            testClassName.c_str(),
            L"SWCH Test Window",
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, 400, 300,
            nullptr, nullptr, hInstance, nullptr
        );
        assert(hwnd != nullptr && "Test window creation must succeed");

        // Verify FindExistingWindow and WaitForExistingWindow find it
        HWND found = swch_single_instance::FindExistingWindow(testClassName.c_str());
        assert(found == hwnd && "FindExistingWindow must locate the created window");

        HWND waited = swch_single_instance::WaitForExistingWindow(testClassName.c_str(), 5, 20);
        assert(waited == hwnd && "WaitForExistingWindow must locate the created window");

        // Minimize window
        ShowWindow(hwnd, SW_MINIMIZE);
        // Process message queue
        MSG msg;
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        assert(IsIconic(hwnd) && "Window should be minimized");

        // ActivateExistingWindow must restore and activate
        bool activated = swch_single_instance::ActivateExistingWindow(hwnd);
        assert(activated && "ActivateExistingWindow must return true for valid window");

        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        assert(!IsIconic(hwnd) && "Window should be restored after activation");

        DestroyWindow(hwnd);
        UnregisterClassW(testClassName.c_str(), hInstance);
    }

    std::cout << "Windows single instance checks passed successfully.\n";
    return 0;
}
