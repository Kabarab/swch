package legendary

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"
)

var binaryPath string

// LegendaryGame structure to parse 'legendary list-games --json' output
type LegendaryGame struct {
	AppName     string            `json:"app_name"`
	AppTitle    string            `json:"app_title"`
	Version     string            `json:"version"`
	IsInstalled bool              `json:"is_installed"`
	InstallPath string            `json:"install_path"`
	Metadata    LegendaryMetadata `json:"metadata"`
}

type LegendaryMetadata struct {
	KeyImages []struct {
		Type   string `json:"type"`
		URL    string `json:"url"`
		Width  int    `json:"width"`
		Height int    `json:"height"`
	} `json:"key_images"`
}

// LegendaryAccountData stores metadata for saved accounts
type LegendaryAccountData struct {
	Name string `json:"name"`
}

// GetLegendaryConfigPath returns the path to the legendary config folder
// Usually ~/.config/legendary or %APPDATA%/legendary
func GetLegendaryConfigPath() string {
	configDir, _ := os.UserConfigDir()
	return filepath.Join(configDir, "legendary")
}

// GetLegendaryStoreDir returns the path where swch stores legendary account backups
func GetLegendaryStoreDir() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch", "legendary_accounts")
	_ = os.MkdirAll(path, 0755)
	return path
}

// ScanLegendaryGames scans the legendary library
func ScanLegendaryGames() []models.LibraryGame {
	var games []models.LibraryGame

	// Check if legendary is in PATH
	path, err := GetBinary()
	if err != nil {
		// If not in PATH, return empty list
		return games
	}

	if !Status() {
		return games
	}

	// Run legendary list-games --json
	cmd := exec.Command(path, "list-games", "--json")
	sys.ConfigureCommand(cmd)
	output, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			fmt.Printf("Error running legendary: %s\nStderr: %s\n", err, string(exitErr.Stderr))
		} else {
			fmt.Println("Error running legendary:", err)
		}
		return games
	}

	var legGames []LegendaryGame
	if err := json.Unmarshal(output, &legGames); err != nil {
		fmt.Println("Error parsing legendary json:", err)
		return games
	}

	// Current implementation assumes the active user is the one logged in
	currentUser := "Legendary User"

	for _, lg := range legGames {
		iconURL := "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg"
		// Try to find a better icon from metadata
		for _, img := range lg.Metadata.KeyImages {
			if img.Type == "DieselGameBox" || img.Type == "Thumbnail" {
				iconURL = img.URL
				break
			}
		}

		games = append(games, models.LibraryGame{
			ID:       lg.AppName,
			Name:     lg.AppTitle,
			Platform: "Legendary", // Use a separate platform ID
			IconURL:  iconURL,
			ExePath:  lg.AppName, // For legendary, the AppID is used for launching
			AvailableOnAccounts: []models.AccountStat{
				{
					AccountID:   "legendary_active",
					DisplayName: "Active Account",
					Username:    currentUser,
					IsHidden:    false,
				},
			},
			IsInstalled: lg.IsInstalled,
		})
	}

	return games
}

// ScanLegendaryAccounts scans saved legendary accounts in swch
func ScanLegendaryAccounts() []models.Account {
	var accounts []models.Account
	baseDir := GetLegendaryStoreDir()

	entries, err := os.ReadDir(baseDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(baseDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				var meta LegendaryAccountData
				d, _ := os.ReadFile(metaPath)
				json.Unmarshal(d, &meta)

				accounts = append(accounts, models.Account{
					ID:          "legendary_" + meta.Name,
					DisplayName: meta.Name,
					Username:    meta.Name,
					Platform:    "Legendary",
				})
			}
		}
	}
	return accounts
}

// SaveCurrentLegendaryAccount saves the current user.json
func SaveCurrentLegendaryAccount(name string) error {
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	configDir := GetLegendaryConfigPath()
	userJsonPath := filepath.Join(configDir, "user.json")

	if _, err := os.Stat(userJsonPath); os.IsNotExist(err) {
		return fmt.Errorf("Legendary user.json not found. Please login using 'legendary auth' first.")
	}

	// Create folder for the account
	destDir := filepath.Join(GetLegendaryStoreDir(), name)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		return err
	}

	// Copy user.json
	destUserJson := filepath.Join(destDir, "user.json")
	if err := copyFileSimple(userJsonPath, destUserJson); err != nil {
		return fmt.Errorf("failed to copy user.json: %v", err)
	}

	// Save metadata
	meta := LegendaryAccountData{Name: name}
	data, _ := json.MarshalIndent(meta, "", "  ")
	return os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644)
}

// SwitchLegendaryAccount swaps the user.json file
func SwitchLegendaryAccount(name string) error {
	storedAccountDir := filepath.Join(GetLegendaryStoreDir(), name)
	storedUserJson := filepath.Join(storedAccountDir, "user.json")

	if _, err := os.Stat(storedUserJson); os.IsNotExist(err) {
		return fmt.Errorf("account backup not found")
	}

	realConfigDir := GetLegendaryConfigPath()
	// Ensure config dir exists
	os.MkdirAll(realConfigDir, 0755)

	realUserJson := filepath.Join(realConfigDir, "user.json")

	// Remove current file to replace it
	os.Remove(realUserJson)

	// Copy the new one
	return copyFileSimple(storedUserJson, realUserJson)
}

// copyFileSimple utility to copy a file
func copyFileSimple(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()

	_, err = io.Copy(out, in)
	return err
}

// LaunchLegendaryAuth launches a terminal for authorization
func LaunchLegendaryAuth() error {
	var cmd *exec.Cmd

	// Command to launch auth
	authCmd := "legendary auth"

	switch runtime.GOOS {
	case "windows":
		// Launch in a new cmd window
		cmd = exec.Command("cmd", "/c", "start", "cmd", "/k", authCmd)
	case "darwin":
		// Launch via Terminal.app
		script := fmt.Sprintf(`tell application "Terminal" to do script "%s"`, authCmd)
		cmd = exec.Command("osascript", "-e", script)
	default:
		// Linux (example for x-terminal-emulator)
		cmd = exec.Command("x-terminal-emulator", "-e", authCmd)
	}

	sys.ConfigureCommand(cmd)
	return cmd.Start()
}

// GetEpicLoginURL returns the URL used for manual login
func GetEpicLoginURL() string {
	return "https://www.epicgames.com/id/api/redirect?clientId=007a63d0b75f46029245e7838e52a85b&responseType=code"
}

// LoginWithBrowser launches the auth flow (alias for LaunchLegendaryAuth)
func LoginWithBrowser() error {
	return LaunchLegendaryAuth()
}

// Путь к бинарнику legendary.
// В Heroic он поставляется вместе с лаунчером, либо ищется в системе.
func GetBinary() (string, error) {
	if binaryPath != "" {
		return binaryPath, nil
	}

	var platformDir string
	var binaryName string

	switch runtime.GOOS {
	case "windows":
		platformDir = "windows"
		binaryName = "legendary.exe"
	case "darwin":
		platformDir = "darwin"
		binaryName = "legendary"
	case "linux":
		platformDir = "linux"
		binaryName = "legendary"
	default:
		binaryName = "legendary"
	}

	// 1. Ищем рядом с исполняемым файлом (для портативности)
	exePath, err := os.Executable()
	if err == nil {
		exeDir := filepath.Dir(exePath)
		// Проверяем: ./tools/windows/legendary.exe
		bundledPath := filepath.Join(exeDir, "tools", platformDir, binaryName)
		if _, err := os.Stat(bundledPath); err == nil {
			binaryPath = bundledPath
			if runtime.GOOS != "windows" {
				_ = os.Chmod(binaryPath, 0755)
			}
			fmt.Println("[Legendary] Found bundled binary:", binaryPath)
			return binaryPath, nil
		}

		// На Mac в готовом приложении (app bundle) путь может быть сложнее,
		// но если вы положите tools рядом с бинарником внутри .app/Contents/MacOS/, это сработает.
		// Для разработки Wails обычно кладет бинарник в build/bin, поэтому ищем там:
		devPath := filepath.Join(exeDir, "..", "..", "tools", platformDir, binaryName)
		if _, err := os.Stat(devPath); err == nil {
			binaryPath = devPath
			if runtime.GOOS != "windows" {
				_ = os.Chmod(binaryPath, 0755)
			}
			fmt.Println("[Legendary] Found dev binary:", binaryPath)
			return binaryPath, nil
		}
	}

	// 2. Ищем в текущей рабочей директории (полезно для `wails dev`)
	wd, err := os.Getwd()
	if err == nil {
		localPath := filepath.Join(wd, "tools", platformDir, binaryName)
		if _, err := os.Stat(localPath); err == nil {
			binaryPath = localPath
			if runtime.GOOS != "windows" {
				_ = os.Chmod(binaryPath, 0755)
			}
			fmt.Println("[Legendary] Found local binary:", binaryPath)
			return binaryPath, nil
		}
	}

	// 3. Если не нашли, пробуем искать в глобальном PATH (на случай если пользователь сам установил)
	path, err := exec.LookPath(binaryName)
	if err == nil {
		binaryPath = path
		fmt.Println("[Legendary] Found binary in PATH:", binaryPath)
		return binaryPath, nil
	}

	return "", fmt.Errorf("legendary binary not found")
}

// Auth - авторизация через SID
func Auth(sid string) error {
	// --sid принудительно пытается войти
	output, err := runCommand("auth", "--sid", sid, "--disable-webview")

	if err != nil {
		// Теперь мы увидим реальную причину (например "Invalid SID")
		return fmt.Errorf("ошибка входа: %s. Вывод: %s", err, string(output))
	}
	return nil
}

// ImportFromEpicLauncher импортирует авторизацию из установленного Epic Games Launcher
// Это позволяет автоматически использовать токены пользователя без повторного входа
func ImportFromEpicLauncher() error {
	bin, err := GetBinary()
	if err != nil {
		return fmt.Errorf("legendary binary not found: %w", err)
	}

	// Проверяем, есть ли данные Epic Launcher
	localAppData := os.Getenv("LOCALAPPDATA")
	if localAppData == "" {
		home, _ := os.UserHomeDir()
		localAppData = filepath.Join(home, "AppData", "Local")
	}

	epicDataDir := filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Data")
	if _, err := os.Stat(epicDataDir); os.IsNotExist(err) {
		return fmt.Errorf("Epic Games Launcher data not found. Please login to Epic Games Launcher first")
	}

	// Импортируем авторизацию
	// ВАЖНО: --import выходит из Epic Launcher, поэтому предупреждаем об этом
	fmt.Println("[Legendary] Importing authentication from Epic Games Launcher...")
	cmd := exec.Command(bin, "auth", "--import")
	sys.ConfigureCommand(cmd)
	output, err := cmd.CombinedOutput()

	if err != nil {
		return fmt.Errorf("failed to import auth: %s, output: %s", err, string(output))
	}

	fmt.Println("[Legendary] Successfully imported Epic Games authentication")
	return nil
}

// EnsureAuthenticated проверяет авторизацию и автоматически импортирует из Epic Launcher если нужно
func EnsureAuthenticated() error {
	// Сначала проверяем, авторизован ли уже Legendary
	if Status() {
		fmt.Println("[Legendary] Already authenticated")
		return nil
	}

	// Если нет — пытаемся импортировать из Epic Launcher
	fmt.Println("[Legendary] Not authenticated, trying to import from Epic Launcher...")
	return ImportFromEpicLauncher()
}

// GetFullLibrary получает полную библиотеку игр (включая не установленные)
// Автоматически импортирует авторизацию из Epic Launcher если нужно
func GetFullLibrary() ([]LegendaryGame, error) {
	// Убеждаемся, что авторизованы
	if err := EnsureAuthenticated(); err != nil {
		return nil, fmt.Errorf("authentication failed: %w", err)
	}

	// Получаем список игр
	bin, err := GetBinary()
	if err != nil {
		return nil, err
	}

	cmd := exec.Command(bin, "list-games", "--json")
	sys.ConfigureCommand(cmd)
	output, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("failed to list games: %w", err)
	}

	var games []LegendaryGame
	if err := json.Unmarshal(output, &games); err != nil {
		return nil, fmt.Errorf("failed to parse games: %w", err)
	}

	return games, nil
}

func Status() bool {
	bin, err := GetBinary()
	if err != nil {
		return false
	}
	// 'legendary status' возвращает 0, если вход выполнен, и 1, если нет (обычно)
	cmd := exec.Command(bin, "status")
	sys.ConfigureCommand(cmd)
	output, err := cmd.CombinedOutput()
	if err != nil {
		return false
	}
	outStr := string(output)
	return !strings.Contains(outStr, "No saved credentials") && !strings.Contains(outStr, "Not logged in")
}

func ListGames() ([]models.EpicGame, error) {
	// --json выводит данные, которые легко парсить
	output, err := runCommand("list-games", "--json")
	if err != nil {
		return nil, err
	}

	var games []models.EpicGame
	if err := json.Unmarshal(output, &games); err != nil {
		return nil, fmt.Errorf("ошибка парсинга JSON: %s", err)
	}

	return games, nil
}

// InstallGame запускает установку игры
func InstallGame(appName string, installPath string) error {
	bin, err := GetBinary()
	if err != nil {
		return err
	}
	args := []string{"install", appName, "-y"}
	if installPath != "" {
		args = append(args, "--base-path", installPath)
	}
	cmd := exec.Command(bin, args...)
	sys.ConfigureCommand(cmd)
	return cmd.Start()
}

// VerifyGame проверяет файлы игры
func VerifyGame(appName string) error {
	bin, err := GetBinary()
	if err != nil {
		return err
	}
	cmd := exec.Command(bin, "verify", appName)
	sys.ConfigureCommand(cmd)
	return cmd.Start()
}

// ImportGame импортирует игру из папки
func ImportGame(appName, path string) error {
	bin, err := GetBinary()
	if err != nil {
		return err
	}
	cmd := exec.Command(bin, "import", appName, path)
	sys.ConfigureCommand(cmd)
	return cmd.Run()
}

// LaunchGame запускает игру
func LaunchGame(appName string) error {
	bin, err := GetBinary()
	if err != nil {
		return err
	}
	cmd := exec.Command(bin, "launch", appName)
	sys.ConfigureCommand(cmd)
	return cmd.Start()
}

// Logout выполняет выход из аккаунта
func Logout() error {
	_, err := runCommand("auth", "--delete")
	return err
}

func runCommand(args ...string) ([]byte, error) {
	bin, err := GetBinary()
	if err != nil {
		return nil, err
	}

	cmd := exec.Command(bin, args...)
	sys.ConfigureCommand(cmd) // Используем sys.ConfigureCommand для кроссплатформенности

	// CombinedOutput возвращает stdout и stderr вместе
	return cmd.CombinedOutput()
}

// DeleteLegendaryAccount удаляет сохраненный аккаунт
func DeleteLegendaryAccount(name string) error {
	if name == "" {
		return fmt.Errorf("имя аккаунта не может быть пустым")
	}
	accountDir := filepath.Join(GetLegendaryStoreDir(), name)
	if _, err := os.Stat(accountDir); os.IsNotExist(err) {
		return fmt.Errorf("аккаунт '%s' не найден", name)
	}
	return os.RemoveAll(accountDir)
}
