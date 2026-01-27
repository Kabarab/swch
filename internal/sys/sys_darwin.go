//go:build darwin

package sys

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// --- STEAM UTILS (macOS) ---

func GetSteamPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	// Стандартный путь Steam на macOS
	return filepath.Join(home, "Library", "Application Support", "Steam"), nil
}

func KillSteam() {
	// Pkill ищет процесс по имени. -i (case insensitive), -l (long name)
	_ = exec.Command("pkill", "-il", "steam").Run()
}

func SetSteamUser(username string) error {
	// На macOS автологин через реестр не работает.
	// Основная работа делается через loginusers.vdf в scanner пакете.
	return nil
}

// --- EPIC GAMES UTILS (macOS) ---

func KillEpic() error {
	return exec.Command("pkill", "-il", "EpicGamesLauncher").Run()
}

func GetEpicAccountId() (string, error) {
	// На Mac идентификаторы хранятся иначе, заглушка
	return "", fmt.Errorf("not implemented on macos")
}

func SetEpicAccountId(accountId string) error {
	return nil
}

func GetEpicManifestsDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data", "Manifests")
}

func GetEpicAuthDataDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data")
}

// --- RIOT GAMES UTILS (macOS) ---

func KillRiot() {
	_ = exec.Command("pkill", "-il", "RiotClient").Run()
	_ = exec.Command("pkill", "-il", "LeagueClient").Run()
	_ = exec.Command("pkill", "-il", "VALORANT").Run()
}

// --- LAUNCHER UTILS (macOS) ---

func StartGame(pathOrUrl string) {
	// Команда open открывает файлы, приложения (.app) и ссылки (steam://)
	cmd := exec.Command("open", pathOrUrl)
	cmd.Start()
}

func RunExecutable(path string) error {
	// Если это .app, open запустит его. Если бинарник - терминал.
	cmd := exec.Command("open", path)
	return cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) error {
	// На Mac запуск с аргументами через open требует флага --args
	// Пример: open -n -a "Riot Client" --args --launch-product=...

	// Если exePath - это путь к .app
	cmdArgs := []string{"-n", exePath, "--args"}
	cmdArgs = append(cmdArgs, args...)

	cmd := exec.Command("open", cmdArgs...)
	return cmd.Start()
}
