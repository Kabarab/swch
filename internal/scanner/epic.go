package scanner

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
	"time"
)

// EpicManifest структура файла .item (манифест игры)
type EpicManifest struct {
	FormatVersion   int    `json:"FormatVersion"`
	AppName         string `json:"AppName"`
	DisplayName     string `json:"DisplayName"`
	InstallLocation string `json:"InstallLocation"`
	MainGameAppName string `json:"MainGameAppName"`
}

// EpicAccountData хранит метаданные сохраненного аккаунта (для Windows)
type EpicAccountData struct {
	Name string `json:"name"`
}

// --- Public API ---

func SaveCurrentEpicAccount(name string) error {
	if runtime.GOOS == "darwin" {
		return saveCurrentEpicAccountDarwin(name)
	}
	return saveCurrentEpicAccountWindows(name)
}

func SwitchEpicAccount(name string) error {
	if runtime.GOOS == "darwin" {
		return switchEpicAccountDarwin(name)
	}
	return switchEpicAccountWindows(name)
}

func DeleteEpicAccount(name string) error {
	if runtime.GOOS == "darwin" {
		return deleteEpicAccountDarwin(name)
	}
	return deleteEpicAccountWindows(name)
}

func ScanEpicAccounts() []models.Account {
	if runtime.GOOS == "darwin" {
		return scanEpicAccountsDarwin()
	}
	return scanEpicAccountsWindows()
}

// --- Unified Implementation ---

func getEpicConfigDir() (string, error) {
	configDir, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	path := filepath.Join(configDir, "swch", "epic_accounts")
	if err := os.MkdirAll(path, 0755); err != nil {
		return "", err
	}
	return path, nil
}

// --- macOS Implementation ---

func saveCurrentEpicAccountDarwin(name string) error {
	fmt.Printf("[Epic] Saving account: %s\n", name)
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	home, _ := os.UserHomeDir()
	baseEpicDir := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher")
	if _, err := os.Stat(baseEpicDir); os.IsNotExist(err) {
		return fmt.Errorf("Epic Games Launcher data not found")
	}

	configDir, err := getEpicConfigDir()
	if err != nil {
		fmt.Printf("[Epic] Error getting config dir: %v\n", err)
		return fmt.Errorf("could not get config dir: %w", err)
	}

	destDir := filepath.Join(configDir, name)
	fmt.Printf("[Epic] Destination Dir: %s\n", destDir)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		fmt.Printf("[Epic] Failed to create dest dir: %v\n", err)
		return err
	}

	// 1. Save Saved/Data (Main Auth) -> dest/Data
	srcSavedData := filepath.Join(baseEpicDir, "Saved", "Data")
	destDataPath := filepath.Join(destDir, "Data")
	_ = os.RemoveAll(destDataPath)

	if _, err := os.Stat(srcSavedData); err == nil {
		if err := copyDir(srcSavedData, destDataPath); err != nil {
			return fmt.Errorf("failed to copy Saved/Data: %v", err)
		}
	}

	// 1.5 Save Saved/Auth (Optional but recommended) -> dest/Auth
	srcSavedAuth := filepath.Join(baseEpicDir, "Saved", "Auth")
	destAuthPath := filepath.Join(destDir, "Auth")
	_ = os.RemoveAll(destAuthPath)

	if _, err := os.Stat(srcSavedAuth); err == nil {
		copyDir(srcSavedAuth, destAuthPath)
	}

	// 2. Save Data (Extra Auth) -> dest/RootData (Exclude Manifests)
	srcRootData := filepath.Join(baseEpicDir, "Data")
	destRootDataPath := filepath.Join(destDir, "RootData")
	_ = os.RemoveAll(destRootDataPath)

	if _, err := os.Stat(srcRootData); err == nil {
		copyDirFiltered(srcRootData, destRootDataPath, []string{"Manifests"})
	}

	// --- NEW: Save Config folder ---
	srcConfigPath := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Saved", "Config")
	destConfigPath := filepath.Join(destDir, "Config")

	fmt.Printf("[Epic] Saving Config from %s to %s\n", srcConfigPath, destConfigPath)
	if _, err := os.Stat(srcConfigPath); err == nil {
		_ = os.RemoveAll(destConfigPath)
		if err := copyDir(srcConfigPath, destConfigPath); err != nil {
			// Warning: Failed to copy config
			fmt.Printf("[Epic] Warning: Failed to copy config: %v\n", err)
		}
	} else {
		fmt.Printf("[Epic] Warning: Source Config dir not found at %s\n", srcConfigPath)
	}

	// --- NEW: Save Plists (macOS Preferences) ---
	savePlists(destDir)

	meta := EpicAccountData{Name: name}
	data, _ := json.MarshalIndent(meta, "", "  ")
	if err := os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644); err != nil {
		return err
	}

	fmt.Println("[Epic] Account saved successfully. Restarting Epic...")
	sys.StartEpic()
	return nil
}

func switchEpicAccountDarwin(name string) error {
	fmt.Printf("[Epic] Switching to account: %s\n", name)
	configDir, err := getEpicConfigDir()
	if err != nil {
		fmt.Printf("[Epic] Error getting config dir: %v\n", err)
		return fmt.Errorf("could not get config dir: %w", err)
	}
	accountDir := filepath.Join(configDir, name)
	fmt.Printf("[Epic] Account directory: %s\n", accountDir)

	if _, err := os.Stat(filepath.Join(accountDir, "meta.json")); os.IsNotExist(err) {
		fmt.Println("[Epic] meta.json not found")
		return fmt.Errorf("account not found")
	}

	fmt.Println("[Epic] Killing Epic Games Launcher...")
	if err := sys.KillEpic(); err != nil {
		fmt.Printf("[Epic] Failed to kill Epic: %v\n", err)
		return fmt.Errorf("failed to kill Epic Games Launcher: %w", err)
	}

	// --- NEW: Clean System State & Keychain ---
	cleanEpicStateDarwin()
	// ОТКЛЮЧАЕМ УДАЛЕНИЕ KEYCHAIN ДЛЯ ТЕСТА GOOGLE LOGIN
	// Если это поможет, значит проблема была именно в удалении токена.
	// deleteEpicKeychainEntry()

	home, _ := os.UserHomeDir()
	baseEpicDir := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher")

	// 1. Restore Saved/Data
	realSavedData := filepath.Join(baseEpicDir, "Saved", "Data")
	_ = os.RemoveAll(realSavedData)

	storedDataPath := filepath.Join(accountDir, "Data")
	if _, err := os.Stat(storedDataPath); err == nil {
		copyDir(storedDataPath, realSavedData)
	}

	// 1.5 Restore Saved/Auth
	realSavedAuth := filepath.Join(baseEpicDir, "Saved", "Auth")
	_ = os.RemoveAll(realSavedAuth)
	storedAuthPath := filepath.Join(accountDir, "Auth")
	if _, err := os.Stat(storedAuthPath); err == nil {
		copyDir(storedAuthPath, realSavedAuth)
	}

	// 2. Restore RootData (preserving Manifests in target)
	realRootData := filepath.Join(baseEpicDir, "Data")
	// We don't delete realRootData fully, we only overwrite/add files,
	// but to be clean we should probably remove everything EXCEPT Manifests.
	// For safety, let's just copy over.
	storedRootDataPath := filepath.Join(accountDir, "RootData")
	if _, err := os.Stat(storedRootDataPath); err == nil {
		copyDir(storedRootDataPath, realRootData)
	}

	// --- NEW: Restore Config folder ---
	realConfigPath := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Saved", "Config")
	storedConfigPath := filepath.Join(accountDir, "Config")

	fmt.Printf("[Epic] Restoring Config from %s to %s\n", storedConfigPath, realConfigPath)
	// Always remove real config to avoid mismatches
	_ = os.RemoveAll(realConfigPath)

	// Check if stored config exists
	if _, err := os.Stat(storedConfigPath); os.IsNotExist(err) {
		fmt.Printf("[Epic] Stored Config not found for account %s\n", name)
		return fmt.Errorf("saved account is missing Config data. Please login manually and re-save this account")
	}

	if err := copyDir(storedConfigPath, realConfigPath); err != nil {
		fmt.Printf("[Epic] Failed to restore config: %v\n", err)
		return fmt.Errorf("failed to restore config: %v", err)
	}

	// --- NEW: Restore Plists (macOS Preferences) ---
	restorePlists(accountDir)

	// --- NEW: Clear Webcache ---
	savedDir := filepath.Join(home, "Library", "Application Support", "Epic", "EpicGamesLauncher", "Saved")
	fmt.Printf("[Epic] Clearing webcache in: %s\n", savedDir)
	entries, _ := os.ReadDir(savedDir)
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "webcache") {
			fmt.Printf("[Epic] Deleting: %s\n", e.Name())
			_ = os.RemoveAll(filepath.Join(savedDir, e.Name()))
		}
	}

	time.Sleep(1 * time.Second) // Даем системе время на обновление кэшей и Keychain
	fmt.Println("[Epic] Starting Epic Games Launcher...")
	sys.StartEpic()
	return nil
}

func deleteEpicAccountDarwin(name string) error {
	configDir, err := getEpicConfigDir()
	if err != nil {
		return err
	}
	return os.RemoveAll(filepath.Join(configDir, name))
}

func scanEpicAccountsDarwin() []models.Account {
	var accounts []models.Account
	configDir, err := getEpicConfigDir()
	if err != nil {
		return accounts
	}

	entries, err := os.ReadDir(configDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(configDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				accounts = append(accounts, models.Account{
					ID:          "epic_" + e.Name(),
					DisplayName: e.Name(),
					Username:    e.Name(),
					Platform:    "Epic",
				})
			}
		}
	}
	return accounts
}

// --- Windows Implementation ---

func saveCurrentEpicAccountWindows(name string) error {
	fmt.Printf("[Epic] Saving account (Win): %s\n", name)
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	srcDataPath := sys.GetEpicAuthDataDir()
	fmt.Printf("[Epic] Source Data: %s\n", srcDataPath)
	if _, err := os.Stat(srcDataPath); os.IsNotExist(err) {
		fmt.Println("[Epic] Source data not found")
		return fmt.Errorf("Epic Data folder not found. Please login to Epic Games Launcher first.")
	}

	configDir, err := getEpicConfigDir()
	if err != nil {
		fmt.Printf("[Epic] Config dir error: %v\n", err)
		return fmt.Errorf("could not get config dir: %w", err)
	}

	destDir := filepath.Join(configDir, name)
	fmt.Printf("[Epic] Dest dir: %s\n", destDir)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		fmt.Printf("[Epic] Mkdir error: %v\n", err)
		return err
	}

	destDataPath := filepath.Join(destDir, "Data")
	fmt.Println("[Epic] Cleaning dest Data...")
	_ = os.RemoveAll(destDataPath)

	fmt.Println("[Epic] Copying Data...")
	if err := copyDir(srcDataPath, destDataPath); err != nil {
		fmt.Printf("[Epic] Copy Data error: %v\n", err)
		return fmt.Errorf("failed to copy auth data: %v", err)
	}

	// --- NEW: Save Config folder ---
	localAppData := os.Getenv("LocalAppData")
	srcConfigPath := filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Config")
	destConfigPath := filepath.Join(destDir, "Config")

	fmt.Printf("[Epic] Saving Config from %s to %s\n", srcConfigPath, destConfigPath)
	if _, err := os.Stat(srcConfigPath); err == nil {
		_ = os.RemoveAll(destConfigPath)
		if err := copyDir(srcConfigPath, destConfigPath); err != nil {
			// Warning: Failed to copy config
			fmt.Printf("[Epic] Copy Config error: %v\n", err)
		}
	} else {
		fmt.Println("[Epic] Source Config not found!")
	}
	// -------------------------------

	meta := EpicAccountData{Name: name}
	data, _ := json.MarshalIndent(meta, "", "  ")
	if err := os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644); err != nil {
		return err
	}

	fmt.Println("[Epic] Save complete. Restarting Epic...")
	sys.StartEpic()
	return nil
}

func switchEpicAccountWindows(name string) error {
	fmt.Printf("[Epic] Switching to (Win): %s\n", name)
	configDir, err := getEpicConfigDir()
	if err != nil {
		fmt.Printf("[Epic] Config dir error: %v\n", err)
		return fmt.Errorf("could not get config dir: %w", err)
	}
	accountDir := filepath.Join(configDir, name)
	fmt.Printf("[Epic] Account dir: %s\n", accountDir)

	if _, err := os.Stat(filepath.Join(accountDir, "meta.json")); os.IsNotExist(err) {
		fmt.Println("[Epic] meta.json missing")
		return fmt.Errorf("account not found")
	}

	fmt.Println("[Epic] Killing Epic...")
	_ = sys.KillEpic()

	realDataPath := sys.GetEpicAuthDataDir()
	fmt.Printf("[Epic] Real Data Path: %s\n", realDataPath)
	fmt.Println("[Epic] Removing real Data...")
	_ = os.RemoveAll(realDataPath)

	storedDataPath := filepath.Join(accountDir, "Data")
	fmt.Printf("[Epic] Copying Data from %s\n", storedDataPath)
	if err := copyDir(storedDataPath, realDataPath); err != nil {
		fmt.Printf("[Epic] Copy Data error: %v\n", err)
		return err
	}

	// --- NEW: Restore Config folder ---
	localAppData := os.Getenv("LocalAppData")
	realConfigPath := filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Config")
	storedConfigPath := filepath.Join(accountDir, "Config")

	fmt.Printf("[Epic] Restoring Config from %s to %s\n", storedConfigPath, realConfigPath)
	// Always remove real config to avoid mismatches
	_ = os.RemoveAll(realConfigPath)

	// Check if stored config exists
	if _, err := os.Stat(storedConfigPath); os.IsNotExist(err) {
		fmt.Println("[Epic] Stored Config missing!")
		return fmt.Errorf("saved account is missing Config data. Please login manually and re-save this account")
	}

	if err := copyDir(storedConfigPath, realConfigPath); err != nil {
		fmt.Printf("[Epic] Copy Config error: %v\n", err)
		return fmt.Errorf("failed to restore config: %v", err)
	}
	// ----------------------------------

	// --- NEW: Clear Webcache ---
	savedDir := filepath.Join(localAppData, "EpicGamesLauncher", "Saved")
	fmt.Printf("[Epic] Clearing webcache in %s\n", savedDir)
	entries, _ := os.ReadDir(savedDir)
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "webcache") {
			fmt.Printf("[Epic] Removing %s\n", e.Name())
			_ = os.RemoveAll(filepath.Join(savedDir, e.Name()))
		}
	}

	fmt.Println("[Epic] Starting Epic...")
	sys.StartEpic()
	return nil
}

func deleteEpicAccountWindows(name string) error {
	configDir, err := getEpicConfigDir()
	if err != nil {
		return err
	}
	return os.RemoveAll(filepath.Join(configDir, name))
}

func scanEpicAccountsWindows() []models.Account {
	var accounts []models.Account
	configDir, err := getEpicConfigDir()
	if err != nil {
		return accounts
	}

	entries, err := os.ReadDir(configDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(configDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				accounts = append(accounts, models.Account{
					ID:          "epic_" + e.Name(),
					DisplayName: e.Name(),
					Username:    e.Name(),
					Platform:    "Epic",
				})
			}
		}
	}
	return accounts
}

// --- Common ---

// ScanEpicGames сканирует установленные игры
func ScanEpicGames() []models.LibraryGame {
	var games []models.LibraryGame
	manifestPath := sys.GetEpicManifestsDir() // Cross-platform

	files, err := os.ReadDir(manifestPath)
	if err != nil {
		return games
	}

	for _, f := range files {
		if strings.HasSuffix(f.Name(), ".item") {
			data, err := os.ReadFile(filepath.Join(manifestPath, f.Name()))
			if err != nil {
				continue
			}

			var manifest EpicManifest
			if err := json.Unmarshal(data, &manifest); err != nil {
				continue
			}

			games = append(games, models.LibraryGame{
				ID:          manifest.AppName,
				Name:        manifest.DisplayName,
				Platform:    "Epic",
				IconURL:     "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:     manifest.InstallLocation,
				IsInstalled: true,
			})
		}
	}
	return games
}

func DeleteAllEpicAccounts() error {
	// This function now works for both OSes as getEpicConfigDir is cross-platform
	configDir, err := getEpicConfigDir()
	if err != nil {
		return err
	}
	return os.RemoveAll(configDir)
}

// --- Utils ---

func copyDir(src string, dst string) error {
	srcInfo, err := os.Stat(src)
	if err != nil {
		return err
	}
	fmt.Printf("Copying dir: %s -> %s\n", src, dst) // Uncomment for very verbose logging
	if err := os.MkdirAll(dst, srcInfo.Mode()); err != nil {
		return err
	}
	entries, err := os.ReadDir(src)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, entry.Name())
		if entry.IsDir() {
			if err := copyDir(srcPath, dstPath); err != nil {
				return err
			}
		} else {
			fmt.Printf("Copying file: %s\n", entry.Name())
			if err := copyFile(srcPath, dstPath); err != nil {
				return err
			}
		}
	}
	return nil
}

func copyFile(src, dst string) error {
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

func savePlists(destDir string) {
	home, _ := os.UserHomeDir()
	prefsDir := filepath.Join(home, "Library", "Preferences")
	plists := []string{
		"com.epicgames.UnrealEngine.Identifiers.plist",
		"com.epicgames.EpicGamesLauncher.plist",
	}

	for _, p := range plists {
		src := filepath.Join(prefsDir, p)
		dst := filepath.Join(destDir, p)
		if _, err := os.Stat(src); err == nil {
			fmt.Printf("[Epic] Saving plist: %s\n", p)
			copyFile(src, dst)
		}
	}
}

func restorePlists(srcDir string) {
	home, _ := os.UserHomeDir()
	prefsDir := filepath.Join(home, "Library", "Preferences")
	plists := []string{
		"com.epicgames.UnrealEngine.Identifiers.plist",
		"com.epicgames.EpicGamesLauncher.plist",
	}

	for _, p := range plists {
		src := filepath.Join(srcDir, p)
		dst := filepath.Join(prefsDir, p)
		if _, err := os.Stat(src); err == nil {
			fmt.Printf("[Epic] Restoring plist: %s\n", p)
			copyFile(src, dst)
		}
	}
	// Force macOS to reload prefs from disk by killing the prefs daemon
	// It restarts automatically. This is more reliable than 'defaults read'.
	exec.Command("killall", "cfprefsd").Run()
}

func cleanEpicStateDarwin() {
	home, _ := os.UserHomeDir()
	// Clear Saved Application State (window positions, etc)
	stateDir := filepath.Join(home, "Library", "Saved Application State", "com.epicgames.EpicGamesLauncher.savedState")
	os.RemoveAll(stateDir)

	// Clear Caches
	cachesDir := filepath.Join(home, "Library", "Caches", "com.epicgames.EpicGamesLauncher")
	os.RemoveAll(cachesDir)
}

func deleteEpicKeychainEntry() {
	// Пытаемся удалить записи из Keychain по всем возможным именам сервисов.
	// Это заставляет лаунчер использовать файлы Data, так как в Keychain пусто.
	services := []string{
		"EpicGamesLauncher",
		"com.epicgames.EpicGamesLauncher",
		"Epic Games Launcher",
		"EpicGames",
	}

	for _, s := range services {
		fmt.Printf("[Epic] Removing keychain entry: %s\n", s)
		if err := exec.Command("security", "delete-generic-password", "-s", s).Run(); err == nil {
			fmt.Printf("[Epic] Deleted generic password for %s\n", s)
		}
		if err := exec.Command("security", "delete-internet-password", "-s", s).Run(); err == nil {
			fmt.Printf("[Epic] Deleted internet password for %s\n", s)
		}
	}
}

func copyDirFiltered(src, dst string, skip []string) error {
	srcInfo, err := os.Stat(src)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dst, srcInfo.Mode()); err != nil {
		return err
	}
	entries, err := os.ReadDir(src)
	if err != nil {
		return err
	}

	skipMap := make(map[string]bool)
	for _, s := range skip {
		skipMap[s] = true
	}

	for _, entry := range entries {
		if skipMap[entry.Name()] {
			continue
		}
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, entry.Name())
		if entry.IsDir() {
			if err := copyDir(srcPath, dstPath); err != nil {
				return err
			}
		} else {
			if err := copyFile(srcPath, dstPath); err != nil {
				return err
			}
		}
	}
	return nil
}
