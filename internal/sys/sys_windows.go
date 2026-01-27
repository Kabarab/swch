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

// ConfigureCommand скрывает окно консоли при запуске команды (только для Windows)
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

	// Сбрасываем активного пользователя, чтобы Steam перечитал конфиг
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
	cmd := exec.Command("taskkill", "/F", "/IM", "EpicGamesLauncher.exe")
	_ = cmd.Run()
	return nil
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

// GetEpicManifestsDir возвращает путь к манифестам игр Epic на Windows
func GetEpicManifestsDir() string {
	programData := os.Getenv("ProgramData")
	if programData == "" {
		programData = "C:\\ProgramData"
	}
	return filepath.Join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests")
}

// GetEpicAuthDataDir возвращает путь к папке с данными авторизации Epic на Windows
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

// --- LAUNCHER UTILS ---

func StartGame(pathOrUrl string) {
	// Если это exe файл, запускаем его напрямую с установкой рабочей директории
	if strings.HasSuffix(strings.ToLower(pathOrUrl), ".exe") {
		RunExecutable(pathOrUrl)
		return
	}
	// Иначе (например, steam:// или URL) запускаем через оболочку cmd
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}

// RunExecutable запускает exe файл, устанавливая его директорию как рабочую.
func RunExecutable(path string) error {
	cleanPath := filepath.Clean(path)
	cmd := exec.Command("explorer", cleanPath)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Start()
}

// StartGameWithArgs запускает exe с аргументами (используется для Riot)
func StartGameWithArgs(exePath string, args ...string) error {
	cmd := exec.Command(exePath, args...)
	cmd.Dir = filepath.Dir(exePath)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Start()
}

// Вспомогательные функции
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
