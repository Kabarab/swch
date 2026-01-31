package legendary

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"swch/internal/models"
	"swch/internal/sys"
)

var binaryPath string
// LegendaryGame structure to parse 'legendary list-games --json' output
type LegendaryGame struct {
	AppName     string `json:"app_name"`
	AppTitle    string `json:"app_title"`
	Version     string `json:"version"`
	IsInstalled bool   `json:"is_installed"`
	InstallPath string `json:"install_path"`
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
	path, err := exec.LookPath("legendary")
	if err != nil {
		// If not in PATH, return empty list
		return games
	}

	// Run legendary list-games --json
	cmd := exec.Command(path, "list-games", "--json")
	sys.ConfigureCommand(cmd)
	output, err := cmd.Output()
	if err != nil {
		fmt.Println("Error running legendary:", err)
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
		games = append(games, models.LibraryGame{
			ID:       lg.AppName,
			Name:     lg.AppTitle,
			Platform: "Legendary", // Use a separate platform ID
			IconURL:  "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
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

	if runtime.GOOS == "windows" {
		// Launch in a new cmd window
		cmd = exec.Command("cmd", "/c", "start", "cmd", "/k", authCmd)
	} else if runtime.GOOS == "darwin" {
		// Launch via Terminal.app
		script := fmt.Sprintf(`tell application "Terminal" to do script "%s"`, authCmd)
		cmd = exec.Command("osascript", "-e", script)
	} else {
		// Linux (example for x-terminal-emulator)
		cmd = exec.Command("x-terminal-emulator", "-e", authCmd)
	}

	sys.ConfigureCommand(cmd)
	return cmd.Start()
}


// Путь к бинарнику legendary. 
// В Heroic он поставляется вместе с лаунчером, либо ищется в системе.
func GetBinary() (string, error) {
	if binaryPath != "" {
		return binaryPath, nil
	}

	binName := "legendary"
	if runtime.GOOS == "windows" {
		binName = "legendary.exe"
	}

	// 1. Ищем рядом с исполняемым файлом (для портативности)
	exePath, err := os.Executable()
	if err == nil {
		localBin := filepath.Join(filepath.Dir(exePath), "tools", binName) // например, папка tools
		if _, err := os.Stat(localBin); err == nil {
			binaryPath = localBin
			return binaryPath, nil
		}
	}

	// 2. Ищем в PATH
	path, err := exec.LookPath(binName)
	if err == nil {
		binaryPath = path
		return binaryPath, nil
	}

	return "", fmt.Errorf("legendary binary not found")
}

// Auth - авторизация через SID
func Auth(sid string) error {
	_, err := runCommand("auth", "--sid", sid)
	return err
}

func Status() bool {
	bin, err := GetBinary()
	if err != nil {
		return false
	}
	// 'legendary status' возвращает 0, если вход выполнен, и 1, если нет (обычно)
	cmd := exec.Command(bin, "status")
	err = cmd.Run()
	return err == nil
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
func InstallGame(appName string) error {
	bin, err := getBinaryPath()
	if err != nil {
		return err
	}
	cmd := exec.Command(bin, "install", appName, "-y")
	setSysProcAttr(cmd)
	return cmd.Start()
}

// LaunchGame запускает игру
func LaunchGame(appName string) error {
	bin, err := getBinaryPath()
	if err != nil {
		return err
	}
	cmd := exec.Command(bin, "launch", appName)
	setSysProcAttr(cmd)
	return cmd.Start()
}

// Logout выполняет выход из аккаунта
func Logout() error {
	_, err := runCommand("auth", "--delete")
	return err
}

func getBinaryPath() (string, error) {
	var platformDir string
	var binaryName string

	// Определяем папку и имя файла в зависимости от системы
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
		return "", fmt.Errorf("неподдерживаемая система: %s", runtime.GOOS)
	}

	// 1. Ищем относительно исполняемого файла (для готовой сборки)
	exePath, err := os.Executable()
	if err == nil {
		exeDir := filepath.Dir(exePath)
		// Проверяем: ./tools/windows/legendary.exe
		bundledPath := filepath.Join(exeDir, "tools", platformDir, binaryName)
		if _, err := os.Stat(bundledPath); err == nil {
			return bundledPath, nil
		}
		
		// На Mac в готовом приложении (app bundle) путь может быть сложнее,
		// но если вы положите tools рядом с бинарником внутри .app/Contents/MacOS/, это сработает.
		// Для разработки Wails обычно кладет бинарник в build/bin, поэтому ищем там:
		devPath := filepath.Join(exeDir, "..", "..", "tools", platformDir, binaryName)
		if _, err := os.Stat(devPath); err == nil {
			return devPath, nil
		}
	}

	// 2. Ищем в текущей рабочей директории (полезно для `wails dev`)
	wd, err := os.Getwd()
	if err == nil {
		localPath := filepath.Join(wd, "tools", platformDir, binaryName)
		if _, err := os.Stat(localPath); err == nil {
			return localPath, nil
		}
	}

	// 3. Если не нашли, пробуем искать в глобальном PATH (на случай если пользователь сам установил)
	path, err := exec.LookPath(binaryName)
	if err == nil {
		return path, nil
	}

	return "", fmt.Errorf("legendary не найден. Ожидался в tools/%s/%s", platformDir, binaryName)
}

func runCommand(args ...string) ([]byte, error) {
	bin, err := getBinaryPath()
	if err != nil {
		return nil, err
	}

	cmd := exec.Command(bin, args...)
	setSysProcAttr(cmd) // Вызов платформо-зависимой функции (скрытие окна)

	output, err := cmd.CombinedOutput()
	if err != nil {
		return output, fmt.Errorf("ошибка legendary: %s, вывод: %s", err, string(output))
	}
	return output, nil
}