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
	// Steam хранит пути с /, Windows любит \
	return filepath.Clean(strings.ReplaceAll(path, "/", "\\")), nil
}

// KillSteam убивает процесс жестко и ждет
func KillSteam() {
	// Убиваем не только Steam, но и его "помощников", которые держат файлы
	targetProcs := []string{"steam.exe", "steamwebhelper.exe"}
	
	for _, p := range targetProcs {
		cmd := exec.Command("taskkill", "/F", "/IM", p)
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		cmd.Run()
	}

	// Ждем 3 секунды, чтобы Windows успела освободить файл реестра.
	// Без этого Steam при закрытии перезапишет нашего пользователя!
	time.Sleep(3 * time.Second)
}

// SetSteamUser меняет пользователя
func SetSteamUser(username string) error {
	if username == "" {
		return fmt.Errorf("empty username")
	}

	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	// 1. Устанавливаем логин
	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}

	// 2. ВАЖНО: 1 должно быть числом (DWORD), иначе Steam попросит пароль
	if err := k.SetDWordValue("RememberPassword", 1); err != nil {
		return err
	}
	
	// Дополнительный флаг для новых версий Steam
	// Убирает предупреждение "Запускается в офлайн режиме" если нет сети
	k.SetDWordValue("SkipOfflineModeWarning", 1) 
	
	return nil
}

func StartGame(pathOrUrl string) {
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}