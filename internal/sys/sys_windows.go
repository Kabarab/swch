//go:build windows

package sys

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows/registry"
)

// ConfigureCommand скрывает окно консоли при запуске команды
func ConfigureCommand(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
}

// --- STEAM UTILS ---

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

func KillSteam() {
	killProcess("steam.exe")
	killProcess("steamwebhelper.exe")
	killProcess("GameOverlayUI.exe")
	waitForExit("steam.exe")
}

func SetSteamUser(username string) error {
	if username == "" {
		return fmt.Errorf("username is empty")
	}

	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}
	if err := k.SetDWordValue("RememberPassword", 1); err != nil {
		return err
	}

	activeKey, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam\ActiveProcess`, registry.SET_VALUE)
	if err == nil {
		defer activeKey.Close()
		_ = activeKey.SetDWordValue("ActiveUser", 0)
		_ = activeKey.SetDWordValue("pid", 0)
	}

	return nil
}

// --- EPIC GAMES UTILS ---

func KillEpic() error {
	// Завершаем все связанные процессы Epic Games
	// Это критично для корректного перезапуска лаунчера при смене аккаунта
	killProcess("EpicGamesLauncher.exe")
	killProcess("EpicWebHelper.exe")
	killProcess("UnrealCEFSubProcess.exe")

	// Ждем завершения основного процесса
	waitForExit("EpicGamesLauncher.exe")
	// Также ждем завершения вспомогательных процессов
	waitForExit("EpicWebHelper.exe")

	// Даем время на освобождение файлов
	time.Sleep(500 * time.Millisecond)
	return nil
}

func getEpicLauncherPath() (string, error) {
	// Пробуем 64-битный путь
	k, err := registry.OpenKey(registry.LOCAL_MACHINE, `SOFTWARE\Epic Games\EpicGamesLauncher`, registry.QUERY_VALUE)
	if err != nil {
		// Если не удалось, пробуем 32-битный путь (для 32-битных приложений на 64-битной ОС)
		k, err = registry.OpenKey(registry.LOCAL_MACHINE, `SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher`, registry.QUERY_VALUE)
		if err != nil {
			return "", fmt.Errorf("cannot find Epic Games Launcher registry key")
		}
	}
	defer k.Close()

	installDir, _, err := k.GetStringValue("InstallDir")
	if err != nil {
		return "", fmt.Errorf("cannot find InstallDir value in registry: %w", err)
	}
	// Путь к исполняемому файлу в современных версиях лаунчера
	return filepath.Join(installDir, "Launcher", "Portal", "Binaries", "Win64", "EpicGamesLauncher.exe"), nil
}

func StartEpic() {
	path, err := getEpicLauncherPath()
	if err != nil {
		fmt.Printf("[Epic] Could not get Epic Launcher path: %v. Trying default path.\n", err)
		// Пробуем стандартные пути как запасной вариант
		defaultPaths := []string{
			"C:\\Program Files (x86)\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
			"C:\\Program Files\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
		}
		found := false
		for _, p := range defaultPaths {
			if _, statErr := os.Stat(p); statErr == nil {
				path = p
				found = true
				break
			}
		}
		if !found {
			fmt.Println("[Epic] Could not find Epic Launcher executable")
			return
		}
	}

	// Проверяем существование файла
	if _, err := os.Stat(path); os.IsNotExist(err) {
		fmt.Printf("[Epic] Epic Launcher not found at: %s\n", path)
		return
	}

	fmt.Printf("[Epic] Starting Epic Launcher from: %s\n", path)

	// Небольшая задержка перед запуском, чтобы дать файловой системе "успокоиться" после Kill
	time.Sleep(2 * time.Second)

	// Используем cmd.exe /C start для более надежного запуска
	// Это лучше работает в эмуляторах и обходит некоторые проблемы с правами
	cmd := exec.Command("cmd", "/C", "start", "", path)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	err = cmd.Start()
	if err != nil {
		fmt.Printf("[Epic] Failed to start via cmd: %v, trying direct start...\n", err)
		// Запасной вариант - прямой запуск
		err = StartGameWithArgs(path)
		if err != nil {
			fmt.Printf("[Epic] Direct start also failed: %v\n", err)
		}
	} else {
		fmt.Println("[Epic] Start command executed successfully.")
	}
}

func GetEpicAccountId() (string, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Epic Games\Unreal Engine\Identifiers`, registry.QUERY_VALUE)
	if err != nil {
		return "", err
	}
	defer k.Close()
	val, _, err := k.GetStringValue("AccountId")
	return val, err
}

func SetEpicAccountId(accountId string) error {
	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Epic Games\Unreal Engine\Identifiers`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()
	return k.SetStringValue("AccountId", accountId)
}

func GetEpicManifestsDir() string {
	programData := os.Getenv("ProgramData")
	if programData == "" {
		programData = "C:\\ProgramData"
	}
	return filepath.Join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests")
}

func GetEpicAuthDataDir() string {
	localAppData := os.Getenv("LOCALAPPDATA")
	return filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Data")
}

// --- RIOT GAMES UTILS ---

func KillRiot() {
	killProcess("RiotClientServices.exe")
	killProcess("LeagueClient.exe")
	killProcess("VALORANT.exe")
	killProcess("RiotClientUx.exe")
	waitForExit("RiotClientServices.exe")
}

func GetRiotPrivateSettingsPath() string {
	localAppData := os.Getenv("LOCALAPPDATA")
	return filepath.Join(localAppData, "Riot Games", "Riot Client", "Data", "RiotClientPrivateSettings.yaml")
}

// --- LAUNCHER UTILS ---

func StartGame(pathOrUrl string) {
	if strings.HasSuffix(strings.ToLower(pathOrUrl), ".exe") {
		RunExecutable(pathOrUrl)
		return
	}
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}

func RunExecutable(path string) error {
	cleanPath := filepath.Clean(path)
	cmd := exec.Command("explorer", cleanPath)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) error {
	cmd := exec.Command(exePath, args...)
	cmd.Dir = filepath.Dir(exePath)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Start()
}

func killProcess(name string) {
	cmd := exec.Command("taskkill", "/F", "/IM", name)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	_ = cmd.Run()
}

func waitForExit(processName string) {
	for i := 0; i < 20; i++ {
		cmd := exec.Command("tasklist")
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		output, _ := cmd.Output()
		if !strings.Contains(string(output), processName) {
			return
		}
		time.Sleep(500 * time.Millisecond)
	}
}
