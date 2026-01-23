package scanner

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"swch/internal/models"
)

type EpicManifest struct {
	FormatVersion       int    `json:"FormatVersion"`
	AppName             string `json:"AppName"`
	DisplayName         string `json:"DisplayName"`
	InstallLocation     string `json:"InstallLocation"`
	MainGameAppName     string `json:"MainGameAppName"`
}

func ScanEpicGames() []models.LibraryGame {
	var games []models.LibraryGame
	
	// 1. Стандартный путь: C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests
	programData := os.Getenv("ProgramData")
	if programData == "" {
		programData = "C:\\ProgramData"
	}
	manifestPath := filepath.Join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests")

	fmt.Println("Scanning Epic manifests at:", manifestPath) // ЛОГ ДЛЯ ОТЛАДКИ

	files, err := os.ReadDir(manifestPath)
	if err != nil {
		fmt.Println("Error reading Epic path:", err)
		return games
	}

	for _, f := range files {
		if strings.HasSuffix(f.Name(), ".item") {
			data, err := os.ReadFile(filepath.Join(manifestPath, f.Name()))
			if err != nil { continue }

			var manifest EpicManifest
			if err := json.Unmarshal(data, &manifest); err != nil { continue }

			fmt.Println("Found Epic Game:", manifest.DisplayName) // ЛОГ

			games = append(games, models.LibraryGame{
				ID:       manifest.AppName,
				Name:     manifest.DisplayName,
				Platform: "Epic",
				IconURL:  "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:  manifest.InstallLocation,
				AvailableOnAccounts: []models.AccountStat{}, 
			})
		}
	}
	return games
}

func ScanEpicAccounts() []models.Account {
	// Заглушка, так как Epic шифрует данные аккаунта
	return []models.Account{
		{
			ID:          "EpicMain",
			DisplayName: "Epic Games User",
			Username:    "Main Profile",
			Platform:    "Epic",
		},
	}
}