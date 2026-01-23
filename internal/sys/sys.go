package sys

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows/registry"
)

// GetSteamPath получает путь к Steam
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
	return filepath.Clean(strings.ReplaceAll(path, "/", "\\")), nil
}

// KillSteam убивает процессы (Аналог TcNo Processes.cs)
func KillSteam() {
	processes := []string{
		"steam.exe",
		"steamwebhelper.exe",
		"GameOverlayUI.exe",
	}

	for _, proc := range processes {
		cmd := exec.Command("taskkill", "/F", "/IM", proc)
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		_ = cmd.Run()
	}

	// TcNo использует небольшую паузу для гарантии освобождения файлов
	time.Sleep(1 * time.Second)
}

// SetSteamUser пишет в реестр (Аналог TcNo Registry.cs)
func SetSteamUser(username string) error {
	if username == "" {
		return fmt.Errorf("username empty")
	}

	keyPath := `Software\Valve\Steam`
	k, _, err := registry.CreateKey(registry.CURRENT_USER, keyPath, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	// 1. Устанавливаем пользователя
	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}
	if err := k.SetDWordValue("RememberPassword", 1); err != nil {
		return err
	}

	// 2. Сбрасываем флаг Offline (как в TcNo)
	_ = k.SetDWordValue("Offline", 0)

	// 3. Очищаем ActiveProcess. Это заставляет Steam перечитать конфиг.
	// TcNo делает это, чтобы Steam "забыл" предыдущую сессию.
	activeKey, err := registry.OpenKey(registry.CURRENT_USER, keyPath+`\ActiveProcess`, registry.SET_VALUE)
	if err == nil {
		defer activeKey.Close()
		_ = activeKey.SetDWordValue("ActiveUser", 0)
		_ = activeKey.SetDWordValue("pid", 0)
	}

	return nil
}

func StartGame(pathOrUrl string) {
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) {
	cmd := exec.Command(exePath, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}