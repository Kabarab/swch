//go:build linux

package sys

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
)

// Linux не является целевой ОС swch (PRD: Windows/macOS), но этот файл нужен,
// чтобы go build/test ./... работали в CI и dev-контейнерах (PRD 4: AI-Driven QA).
// Steam-функции реализованы по-настоящему (Steam на Linux существует),
// остальные лаунчеры — заглушки.

// ConfigureCommand на Linux не требует специальных настроек
func ConfigureCommand(cmd *exec.Cmd) {
	// No-op
}

// --- STEAM UTILS (Linux) ---

func GetSteamPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	candidates := []string{
		filepath.Join(home, ".local", "share", "Steam"),
		filepath.Join(home, ".steam", "steam"),
	}
	for _, p := range candidates {
		if info, err := os.Stat(p); err == nil && info.IsDir() {
			return p, nil
		}
	}
	return "", fmt.Errorf("steam path not found")
}

func KillSteam() {
	exec.Command("pkill", "-x", "steam").Run()
}

// steamRegistryPath возвращает путь к registry.vdf (Linux-аналог реестра Windows)
func steamRegistryPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	candidates := []string{
		filepath.Join(home, ".steam", "registry.vdf"),
		filepath.Join(home, ".local", "share", "Steam", "registry.vdf"),
	}
	for _, p := range candidates {
		if _, err := os.Stat(p); err == nil {
			return p, nil
		}
	}
	return "", fmt.Errorf("registry.vdf not found")
}

func SetSteamUser(username string) error {
	regPath, err := steamRegistryPath()
	if err != nil {
		return err
	}

	contentBytes, err := os.ReadFile(regPath)
	if err != nil {
		return fmt.Errorf("registry.vdf not readable: %w", err)
	}
	content := string(contentBytes)

	reLogin := regexp.MustCompile(`(?i)"AutoLoginUser"\s+"[^"]*"`)
	newLoginVal := fmt.Sprintf(`"AutoLoginUser"		"%s"`, username)

	if reLogin.MatchString(content) {
		content = reLogin.ReplaceAllString(content, newLoginVal)
	} else {
		reSteamBlock := regexp.MustCompile(`(?i)"Steam"\s*\{`)
		if loc := reSteamBlock.FindStringIndex(content); loc != nil {
			insertStr := fmt.Sprintf("\n\t\t%s", newLoginVal)
			content = content[:loc[1]] + insertStr + content[loc[1]:]
		}
	}

	reRemember := regexp.MustCompile(`(?i)"RememberPassword"\s+"\d+"`)
	if reRemember.MatchString(content) {
		content = reRemember.ReplaceAllString(content, `"RememberPassword"		"1"`)
	}

	return os.WriteFile(regPath, []byte(content), 0644)
}

func GetActiveSteamUser() string {
	regPath, err := steamRegistryPath()
	if err != nil {
		return ""
	}
	contentBytes, err := os.ReadFile(regPath)
	if err != nil {
		return ""
	}
	re := regexp.MustCompile(`(?i)"AutoLoginUser"\s+"([^"]*)"`)
	matches := re.FindStringSubmatch(string(contentBytes))
	if len(matches) > 1 {
		return matches[1]
	}
	return ""
}

// --- EPIC GAMES UTILS (Linux: нативного лаунчера нет, заглушки) ---

func KillEpic() error {
	return nil
}

func GetEpicAuthDataDir() string {
	return ""
}

func GetEpicManifestsDir() string {
	return ""
}

func StartEpic() {
	// No-op
}

func GetEpicAccountId() (string, error) {
	return "", fmt.Errorf("not implemented")
}

func SetEpicAccountId(accountId string) error {
	return nil
}

// --- RIOT GAMES UTILS (Linux: заглушки) ---

func KillRiot() {
	// No-op
}

func GetRiotPrivateSettingsPath() string {
	return ""
}

// --- LAUNCHER UTILS (Linux) ---

func StartGame(pathOrUrl string) {
	exec.Command("xdg-open", pathOrUrl).Start()
}

func RunExecutable(path string) error {
	cmd := exec.Command(path)
	cmd.Dir = filepath.Dir(path)
	return cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) error {
	cmd := exec.Command(exePath, args...)
	cmd.Dir = filepath.Dir(exePath)
	return cmd.Start()
}

func IsProcessRunning(nameOrPath string) bool {
	if nameOrPath == "" {
		return false
	}
	return exec.Command("pgrep", "-f", nameOrPath).Run() == nil
}
