package sys

import (
	"os/exec"
	"path/filepath"
	"syscall"

	"golang.org/x/sys/windows/registry"
)

// GetSteamPath находит путь к Steam
func GetSteamPath() (string, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.QUERY_VALUE)
	if err != nil {
		return "", err
	}
	defer k.Close()

	path, _, err := k.GetStringValue("SteamPath")
	if err != nil {
		return "", err
	}
	return filepath.Clean(path), nil
}

// KillSteam убивает процесс
func KillSteam() {
	// /F - принудительно, /IM - по имени
	cmd := exec.Command("taskkill", "/F", "/IM", "steam.exe")
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Run()
}

// SetSteamUser меняет пользователя (ИСПРАВЛЕНО)
func SetSteamUser(username string) error {
	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	// 1. Устанавливаем логин (строка)
	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}

	// 2. ВАЖНО: Устанавливаем "Запомнить пароль" как ЧИСЛО (DWORD), а не строку!
	// Без этого Steam будет просить пароль при каждом переключении.
	if err := k.SetDWordValue("RememberPassword", 1); err != nil {
		return err
	}
	
	return nil
}

// StartGame запускает EXE или URL
func StartGame(pathOrUrl string) {
	// Используем cmd /C start, чтобы запускать и файлы, и ссылки (steam://)
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}