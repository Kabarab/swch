//go:build darwin

package sys

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"time"
)

// ConfigureCommand для macOS ничего не делает
func ConfigureCommand(cmd *exec.Cmd) {
	// No-op
}

// --- STEAM UTILS (macOS) ---

func GetSteamPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, "Library", "Application Support", "Steam"), nil
}

func KillSteam() {
	// 1. Пытаемся закрыть мягко
	exec.Command("pkill", "-il", "steam").Run()

	// 2. Ждем 3 секунды и добиваем жестко, если еще жив
	done := make(chan error, 1)
	go func() {
		// Проверяем наличие процесса steam_osx (основной бинарник на Mac)
		for i := 0; i < 10; i++ {
			if err := exec.Command("pgrep", "-x", "steam_osx").Run(); err != nil {
				// pgrep вернул ошибку -> процесс не найден -> успех
				done <- nil
				return
			}
			time.Sleep(300 * time.Millisecond)
		}
		// Если все еще жив - убиваем жестко
		exec.Command("killall", "-9", "steam_osx").Run()
		exec.Command("pkill", "-9", "-il", "steam").Run()
		done <- nil
	}()
	<-done
}

func SetSteamUser(username string) error {
	home, err := os.UserHomeDir()
	if err != nil {
		return err
	}
	regPath := filepath.Join(home, "Library", "Application Support", "Steam", "registry.vdf")

	// Читаем файл
	contentBytes, err := os.ReadFile(regPath)
	if err != nil {
		return fmt.Errorf("registry.vdf not found: %v", err)
	}
	content := string(contentBytes)

	// 1. Обновляем AutoLoginUser
	// Используем (?i) для игнорирования регистра (AutoLoginUser или autologinuser)
	reLogin := regexp.MustCompile(`(?i)"AutoLoginUser"\s+"[^"]*"`)
	newLoginVal := fmt.Sprintf(`"AutoLoginUser"		"%s"`, username)

	if reLogin.MatchString(content) {
		content = reLogin.ReplaceAllString(content, newLoginVal)
	} else {
		// Если ключа нет, это проблема. Попробуем вставить его в блок Steam
		// Ищем "Steam" { и вставляем после него
		fmt.Println("[macOS] AutoLoginUser key missing, trying to inject...")
		reSteamBlock := regexp.MustCompile(`(?i)"Steam"\s*\{`)
		if loc := reSteamBlock.FindStringIndex(content); loc != nil {
			insertStr := fmt.Sprintf("\n\t\t%s", newLoginVal)
			content = content[:loc[1]] + insertStr + content[loc[1]:]
		}
	}

	// 2. Обязательно обновляем RememberPassword, иначе автологин не сработает
	reRemember := regexp.MustCompile(`(?i)"RememberPassword"\s+"\d+"`)
	newRememberVal := `"RememberPassword"		"1"`

	if reRemember.MatchString(content) {
		content = reRemember.ReplaceAllString(content, newRememberVal)
	}

	return os.WriteFile(regPath, []byte(content), 0644)
}

// --- EPIC GAMES UTILS (macOS) ---

func KillEpic() error {
	// ИСПРАВЛЕНО: убран флаг -l, который предотвращал закрытие процесса
	return exec.Command("pkill", "-i", "EpicGamesLauncher").Run()
}

func GetEpicAccountId() (string, error) {
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

func GetRiotPrivateSettingsPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Riot Games", "Riot Client", "Data", "RiotClientPrivateSettings.yaml")
}

// --- LAUNCHER UTILS (macOS) ---

func StartGame(pathOrUrl string) {
	exec.Command("open", pathOrUrl).Start()
}

func RunExecutable(path string) error {
	return exec.Command("open", path).Start()
}

func StartGameWithArgs(exePath string, args ...string) error {
	cmdArgs := []string{"-n", exePath, "--args"}
	cmdArgs = append(cmdArgs, args...)
	return exec.Command("open", cmdArgs...).Start()
}