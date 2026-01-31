package epic

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

// Путь к данным Epic Games
func getEpicDataPath() (string, error) {
	if runtime.GOOS == "windows" {
		localAppData, err := os.UserCacheDir() // Обычно возвращает AppData/Local
		if err != nil {
			return "", err
		}
		// На Windows: AppData/Local/EpicGamesLauncher/Saved
		return filepath.Join(localAppData, "EpicGamesLauncher", "Saved"), nil
	} else if runtime.GOOS == "darwin" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		// На Mac: ~/Library/Application Support/Epic/EpicGamesLauncher/Saved
		return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Saved"), nil
	}
	return "", fmt.Errorf("unsupported os")
}

// KillEpic убивает процесс лаунчера перед сменой файлов
func KillEpic() error {
	var cmd *exec.Cmd
	if runtime.GOOS == "windows" {
		cmd = exec.Command("taskkill", "/F", "/IM", "EpicGamesLauncher.exe")
	} else {
		cmd = exec.Command("pkill", "EpicGamesLauncher")
	}
	return cmd.Run()
}

// SwitchAccount меняет папку Config для смены аккаунта
// accountName - имя папки, например "Main", "Alt"
func SwitchAccount(accountName string) error {
	basePath, err := getEpicDataPath()
	if err != nil {
		return err
	}

	configPath := filepath.Join(basePath, "Config")
	
	// 1. Убиваем Epic
	_ = KillEpic()

	// 2. Проверяем, существует ли целевая папка (например Config_Main)
	targetConfig := filepath.Join(basePath, "Config_"+accountName)
	
	// Если мы хотим переключиться на аккаунт, которого нет в бэкапах,
	// но текущая папка Config существует, нам нужно её сохранить под каким-то именем?
	// Упрощенная логика: 
	// Мы предполагаем, что пользователь нажал "Сохранить текущий как AccountA"
	// А потом нажал "Загрузить AccountB"
	
	// Здесь логика зависит от того, как вы хотите организовать UX.
	// Вот пример загрузки:
	
	// Если текущая папка Config существует, переименуем её во временную или "LastUsed"
	if _, err := os.Stat(configPath); err == nil {
		_ = os.Rename(configPath, filepath.Join(basePath, "Config_LastUsed"))
	}

	// Переименовываем сохраненную папку обратно в Config
	if _, err := os.Stat(targetConfig); err == nil {
		err := os.Rename(targetConfig, configPath)
		if err != nil {
			return fmt.Errorf("не удалось активировать аккаунт: %v", err)
		}
	} else {
		return fmt.Errorf("аккаунт %s не найден", accountName)
	}

	return nil
}

// SaveCurrentAccount сохраняет текущую конфигурацию под именем
func SaveCurrentAccount(name string) error {
	basePath, err := getEpicDataPath()
	if err != nil {
		return err
	}
	
	configPath := filepath.Join(basePath, "Config")
	targetPath := filepath.Join(basePath, "Config_"+name)

	// Копируем папку (в Go нет простого CopyDir, нужно использовать Walk или exec)
	// Для простоты на Windows/Mac можно использовать команду cp
	if runtime.GOOS == "windows" {
		// xcopy /E /I source dest
		cmd := exec.Command("xcopy", configPath, targetPath, "/E", "/I", "/Y")
		return cmd.Run()
	} else {
		cmd := exec.Command("cp", "-r", configPath, targetPath)
		return cmd.Run()
	}
}