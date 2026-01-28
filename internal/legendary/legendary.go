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
