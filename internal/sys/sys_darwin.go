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

// ConfigureCommand для macOS не требует специальных настроек (в отличие от Windows)
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

	// 2. Ждем и добиваем жестко, если еще жив
	done := make(chan error, 1)
	go func() {
		for i := 0; i < 10; i++ {
			// pgrep возвращает 0 (успех), если процесс найден
			if err := exec.Command("pgrep", "-x", "steam_osx").Run(); err != nil {
				// Процесс не найден -> успех
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

	contentBytes, err := os.ReadFile(regPath)
	if err != nil {
		return fmt.Errorf("registry.vdf not found: %v", err)
	}
	content := string(contentBytes)

	// 1. Обновляем AutoLoginUser
	reLogin := regexp.MustCompile(`(?i)"AutoLoginUser"\s+"[^"]*"`)
	newLoginVal := fmt.Sprintf(`"AutoLoginUser"		"%s"`, username)

	if reLogin.MatchString(content) {
		content = reLogin.ReplaceAllString(content, newLoginVal)
	} else {
		// Если ключа нет, пробуем вставить его
		reSteamBlock := regexp.MustCompile(`(?i)"Steam"\s*\{`)
		if loc := reSteamBlock.FindStringIndex(content); loc != nil {
			insertStr := fmt.Sprintf("\n\t\t%s", newLoginVal)
			content = content[:loc[1]] + insertStr + content[loc[1]:]
		}
	}

	// 2. Обязательно обновляем RememberPassword
	reRemember := regexp.MustCompile(`(?i)"RememberPassword"\s+"\d+"`)
	newRememberVal := `"RememberPassword"		"1"`

	if reRemember.MatchString(content) {
		content = reRemember.ReplaceAllString(content, newRememberVal)
	}

	return os.WriteFile(regPath, []byte(content), 0644)
}

// --- EPIC GAMES UTILS (macOS) ---

func KillEpic() error {
	// 1. Агрессивно убиваем ВСЕ процессы Epic.
	// Используем 'pkill -9 -f', чтобы убить процесс мгновенно (SIGKILL).
	// -f ищет по всей командной строке, чтобы поймать "EpicGamesLauncher-Mac-Shipping" и "EpicWebHelper".
	
	// Убиваем веб-хелперы (часто именно они держат сессию)
	exec.Command("pkill", "-9", "-f", "EpicWebHelper").Run()
	// Убиваем основной процесс
	exec.Command("pkill", "-9", "-f", "EpicGamesLauncher").Run()

	// 2. Цикл ожидания полного завершения (до 2 секунд)
	// Если мы начнем копировать файлы пока процесс умирает, он может перезаписать их перед смертью.
	for i := 0; i < 20; i++ { 
		// pgrep возвращает exit code 1 (ошибка), если процесс НЕ найден -> значит он закрылся
		if err := exec.Command("pgrep", "-f", "EpicGamesLauncher").Run(); err != nil {
			// Дополнительная пауза, чтобы файловая система "отпустила" файлы
			time.Sleep(200 * time.Millisecond)
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}

	// Если спустя 2 секунды процесс всё еще висит
	return fmt.Errorf("epic games process refuses to die")
}

func GetEpicAuthDataDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data")
}

func GetEpicManifestsDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data", "Manifests")
}

// Заглушки для совместимости с интерфейсом (ID получается через парсинг файлов в scanner)
func GetEpicAccountId() (string, error) {
	return "", fmt.Errorf("not implemented")
}

func SetEpicAccountId(accountId string) error {
	return nil
}

// --- RIOT GAMES UTILS (macOS) ---

func KillRiot() {
	exec.Command("pkill", "-il", "RiotClient").Run()
	exec.Command("pkill", "-il", "LeagueClient").Run()
	exec.Command("pkill", "-il", "VALORANT").Run()
}

func GetRiotPrivateSettingsPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Riot Games", "Riot Client", "Data", "RiotClientPrivateSettings.yaml")
}

// --- LAUNCHER UTILS (macOS) ---

func StartGame(pathOrUrl string) {
	// -n открывает новый экземпляр (помогает, если предыдущий завис)
	exec.Command("open", "-n", pathOrUrl).Start()
}

func RunExecutable(path string) error {
	return exec.Command("open", "-n", path).Start()
}

func StartGameWithArgs(exePath string, args ...string) error {
	cmdArgs := []string{"-n", exePath, "--args"}
	cmdArgs = append(cmdArgs, args...)
	return exec.Command("open", cmdArgs...).Start()
}