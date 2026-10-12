#include "../src/windows_startup.h"
#include <cassert>
#include <iostream>

int main() {
    const std::wstring key = L"Software\\SWCH\\Tests\\Startup-" + std::to_wstring(GetCurrentProcessId());
    HKEY testKey = nullptr;
    assert(RegCreateKeyExW(HKEY_CURRENT_USER, key.c_str(), 0, nullptr, REG_OPTION_VOLATILE,
                          KEY_ALL_ACCESS, nullptr, &testKey, nullptr) == ERROR_SUCCESS);
    const auto cleanup = [&] { RegCloseKey(testKey); RegDeleteKeyW(HKEY_CURRENT_USER, key.c_str()); };
    struct Cleanup { decltype(cleanup)& fn; ~Cleanup() { fn(); } } guard{cleanup};
    const std::wstring exe = L"C:\\Игры и программы\\SWCH\\swch.exe";
    std::wstring mode;
    assert(swch_startup::ReadMode(mode, key.c_str()) == ERROR_SUCCESS && mode == L"off");
    for (const auto* desired : {L"open", L"minimized", L"off", L"off"}) {
        assert(swch_startup::WriteMode(desired, exe, key.c_str()) == ERROR_SUCCESS);
        assert(swch_startup::ReadMode(mode, key.c_str()) == ERROR_SUCCESS && mode == desired);
        if (mode != L"off") {
            wchar_t command[261] = {};
            DWORD bytes = sizeof(command);
            assert(RegGetValueW(testKey, nullptr, L"SWCH", RRF_RT_REG_SZ, nullptr, command, &bytes) == ERROR_SUCCESS);
            int argc = 0;
            auto argv = CommandLineToArgvW(command, &argc);
            assert(argv && argv[0] == exe);
            assert(argc == (mode == L"minimized" ? 2 : 1));
            assert(swch_startup::HasMinimizedArgument(argc, argv) == (mode == L"minimized"));
            LocalFree(argv);
        }
    }
    // A bad mode/path must leave the previous setting intact.
    assert(swch_startup::WriteMode(L"open", exe, key.c_str()) == ERROR_SUCCESS);
    assert(swch_startup::WriteMode(L"unknown", exe, key.c_str()) == ERROR_INVALID_PARAMETER);
    assert(swch_startup::WriteMode(L"minimized", std::wstring(260, L'x'), key.c_str()) == ERROR_FILENAME_EXCED_RANGE);
    assert(swch_startup::WriteMode(L"open", L"", key.c_str()) == ERROR_INVALID_PARAMETER);
    assert(swch_startup::ReadMode(mode, key.c_str()) == ERROR_SUCCESS && mode == L"open");
    // Turning off SWCH must preserve other Run values.
    DWORD other = 42;
    assert(RegSetValueExW(testKey, L"OtherApp", 0, REG_DWORD, reinterpret_cast<BYTE*>(&other), sizeof(other)) == ERROR_SUCCESS);
    assert(swch_startup::WriteMode(L"off", exe, key.c_str()) == ERROR_SUCCESS);
    DWORD bytes = sizeof(other);
    assert(RegGetValueW(testKey, nullptr, L"OtherApp", RRF_RT_REG_DWORD, nullptr, &other, &bytes) == ERROR_SUCCESS && other == 42);
    // The flag must be an argument, not a substring in the executable's path.
    assert(swch_startup::WriteMode(L"open", L"C:\\--start-minimized\\swch.exe", key.c_str()) == ERROR_SUCCESS);
    assert(swch_startup::ReadMode(mode, key.c_str()) == ERROR_SUCCESS && mode == L"open");
    assert(RegSetValueExW(testKey, L"SWCH", 0, REG_DWORD, reinterpret_cast<BYTE*>(&other), sizeof(other)) == ERROR_SUCCESS);
    assert(swch_startup::ReadMode(mode, key.c_str()) == ERROR_UNSUPPORTED_TYPE);
    std::cout << "Windows startup registry and command-line checks passed.\n";
}
