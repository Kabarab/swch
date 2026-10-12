#pragma once

#include <windows.h>
#include <shellapi.h>
#include <string>

namespace swch_startup {
constexpr const wchar_t* kRunKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
constexpr const wchar_t* kValueName = L"SWCH";

inline bool ValidMode(const std::wstring& mode) {
    return mode == L"off" || mode == L"open" || mode == L"minimized";
}

inline bool HasMinimizedArgument(int argc, wchar_t* const* argv) {
    for (int i = 1; i < argc; ++i) {
        if (wcscmp(argv[i], L"--start-minimized") == 0) return true;
    }
    return false;
}

// The optional key/name let tests use a private, non-autostart registry key.
inline LSTATUS ReadMode(std::wstring& mode, const wchar_t* key = kRunKey,
                        const wchar_t* name = kValueName) {
    mode = L"off";
    wchar_t command[261] = {};
    DWORD bytes = sizeof(command);
    LSTATUS result = RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_SZ,
                                 nullptr, command, &bytes);
    if (result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND) return ERROR_SUCCESS;
    if (result != ERROR_SUCCESS) return result;
    if (!command[0]) return ERROR_SUCCESS;
    int argc = 0;
    auto argv = CommandLineToArgvW(command, &argc);
    if (!argv) return GetLastError();
    mode = HasMinimizedArgument(argc, argv) ? L"minimized" : L"open";
    LocalFree(argv);
    return ERROR_SUCCESS;
}

inline LSTATUS WriteMode(const std::wstring& mode, const std::wstring& exe,
                         const wchar_t* key = kRunKey, const wchar_t* name = kValueName) {
    if (!ValidMode(mode)) return ERROR_INVALID_PARAMETER;
    HKEY runKey = nullptr;
    LSTATUS result;
    if (mode == L"off") {
        result = RegOpenKeyExW(HKEY_CURRENT_USER, key, 0, KEY_SET_VALUE, &runKey);
        if (result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND) return ERROR_SUCCESS;
        if (result != ERROR_SUCCESS) return result;
        result = RegDeleteValueW(runKey, name);
        RegCloseKey(runKey);
        return result == ERROR_FILE_NOT_FOUND ? ERROR_SUCCESS : result;
    }
    if (exe.empty() || exe.find(L'"') != std::wstring::npos) return ERROR_INVALID_PARAMETER;
    const std::wstring command = L"\"" + exe + L"\"" +
        (mode == L"minimized" ? L" --start-minimized" : L"");
    // Windows Run entries support command lines of at most 260 characters.
    if (command.size() > 260) return ERROR_FILENAME_EXCED_RANGE;
    result = RegCreateKeyExW(HKEY_CURRENT_USER, key, 0, nullptr, 0,
                             KEY_SET_VALUE, nullptr, &runKey, nullptr);
    if (result != ERROR_SUCCESS) return result;
    result = RegSetValueExW(runKey, name, 0, REG_SZ,
        reinterpret_cast<const BYTE*>(command.c_str()),
        static_cast<DWORD>((command.size() + 1) * sizeof(wchar_t)));
    RegCloseKey(runKey);
    return result;
}
} // namespace swch_startup
