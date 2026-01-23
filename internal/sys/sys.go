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

func GetSteamPath() (string, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.QUERY_VALUE)
	if err != nil { return "", err }
	defer k.Close()
	path, _, err := k.GetStringValue("SteamPath")
	if err != nil { return "", err }
	return filepath.Clean(strings.ReplaceAll(path, "/", "\\")), nil
}

func KillSteam() {
	// Убиваем дерево процессов
	exec.Command("taskkill", "/F", "/IM", "steam.exe").Run()
	exec.Command("taskkill", "/F", "/IM", "steamwebhelper.exe").Run()
	// Даем время на запись файлов и освобождение реестра
	time.Sleep(2 * time.Second)
}

func SetSteamUser(username string) error {
	if username == "" || username == "UNKNOWN" {
		return fmt.Errorf("invalid username")
	}

	keyPath := `Software\Valve\Steam`
	k, _, err := registry.CreateKey(registry.CURRENT_USER, keyPath, registry.SET_VALUE)
	if err != nil { return err }
	defer k.Close()

	// 1. Устанавливаем целевого пользователя
	if err := k.SetStringValue("AutoLoginUser", username); err != nil { return err }
	if err := k.SetDWordValue("RememberPassword", 1); err != nil { return err }

	// 2. КРИТИЧЕСКИ ВАЖНО ДЛЯ НОВОГО STEAM:
	// Удаляем запись об активном пользователе, чтобы Steam не пытался возобновить прошлую сессию
	// ActiveProcess хранит PID активного стима. Если его удалить, Steam думает что это чистый запуск.
	activeKey, err := registry.OpenKey(registry.CURRENT_USER, keyPath+`\ActiveProcess`, registry.SET_VALUE)
	if err == nil {
		defer activeKey.Close()
		// Сбрасываем ActiveUser, чтобы заставить Steam прочитать AutoLoginUser
		activeKey.SetDWordValue("ActiveUser", 0) 
	}

	return nil
}

func StartGame(pathOrUrl string) {
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}