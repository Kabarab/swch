//go:build darwin

package sys

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

// ConfigureCommand для macOS не требует специальных настроек (в отличие от Windows)
func ConfigureCommand(cmd *exec.Cmd) {
	// No-op
}

// ListProcessPaths возвращает пути исполняемых файлов всех запущенных процессов.
// Используется трекером игрового времени.
func ListProcessPaths() ([]string, error) {
	out, err := exec.Command("ps", "-axo", "comm=").Output()
	if err != nil {
		return nil, fmt.Errorf("failed to list processes: %w", err)
	}
	var paths []string
	for _, line := range strings.Split(string(out), "\n") {
		line = strings.TrimSpace(line)
		if line != "" {
			paths = append(paths, line)
		}
	}
	return paths, nil
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

	if _, err := os.Stat(regPath); os.IsNotExist(err) {
		regPath = filepath.Join(home, "Library", "Application Support", "Steam", "config", "registry.vdf")
	}

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
	// 1. Попытка graceful shutdown (SIGTERM)
	// -15 = SIGTERM. Даем приложению шанс сохранить конфиги (GameUserSettings.ini, токены)
	fmt.Println("[Epic] Sending SIGTERM (Graceful Quit)...")
	exec.Command("pkill", "-15", "-f", "EpicGamesLauncher").Run()
	exec.Command("pkill", "-15", "-f", "Epic Games Launcher").Run()

	// 2. Ждем, пока процесс закроется (до 5 секунд)
	// Epic Games может долго писать на диск
	for i := 0; i < 50; i++ { // 50 * 100ms = 5 sec
		err1 := exec.Command("pgrep", "-f", "EpicGamesLauncher").Run()
		err2 := exec.Command("pgrep", "-f", "Epic Games Launcher").Run()
		// Если pgrep вернул ошибку, значит процесс НЕ найден -> он закрылся
		if err1 != nil && err2 != nil {
			fmt.Println("[Epic] Process exited gracefully.")
			// Небольшая доп. пауза для гарантии записи файлов
			time.Sleep(500 * time.Millisecond)
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}

	fmt.Println("[Epic] Process refused to quit. Forcing SIGKILL...")

	// 3. Если не закрылся — убиваем жестко (Cleanup)
	exec.Command("pkill", "-9", "-f", "EpicWebHelper").Run()
	exec.Command("pkill", "-9", "-f", "EpicOnlineServices").Run()
	exec.Command("pkill", "-9", "-f", "com.epicgames.EpicOnlineServices").Run()
	exec.Command("pkill", "-9", "-f", "EpicGamesLauncher").Run()
	exec.Command("pkill", "-9", "-f", "Epic Games Launcher").Run()

	// 4. Финальная проверка
	for i := 0; i < 20; i++ {
		err1 := exec.Command("pgrep", "-f", "EpicGamesLauncher").Run()
		err2 := exec.Command("pgrep", "-f", "Epic Games Launcher").Run()
		if err1 != nil && err2 != nil {
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}

	return fmt.Errorf("epic games process refuses to die")
}

func GetEpicAuthDataDir() string {
	home, _ := os.UserHomeDir()
	// На macOS токены часто лежат прямо в Data, а не в Saved/Data
	// Но проверим оба варианта, чтобы не сломать, если структура изменится
	savedData := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Saved", "Data")
	if info, err := os.Stat(savedData); err == nil && info.IsDir() {
		return savedData
	}
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data")
}

func GetEpicManifestsDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Data", "Manifests")
}

func StartEpic() {
	// -SkipBuildPatchPrereq: пропускает проверку обновлений при запуске
	StartGameWithArgs("/Applications/Epic Games Launcher.app", "-SkipBuildPatchPrereq")
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
