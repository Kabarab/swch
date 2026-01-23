package scanner

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"swch/internal/models"
)

type EpicManifest struct {
	FormatVersion       int    `json:"FormatVersion"`
	AppName             string `json:"AppName"` // Это ID игры
	DisplayName         string `json:"DisplayName"`
	InstallLocation     string `json:"InstallLocation"`
	MainGameAppName     string `json:"MainGameAppName"`
}

func ScanEpicGames() []models.LibraryGame {
	var games []models.LibraryGame
	
	// Путь к манифестам Epic Games (обычно скрытая папка ProgramData)
	programData := os.Getenv("ProgramData")
	if programData == "" {
		programData = "C:\\ProgramData"
	}
	manifestPath := filepath.Join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests")

	files, err := os.ReadDir(manifestPath)
	if err != nil {
		return games
	}

	for _, f := range files {
		if filepath.Ext(f.Name()) == ".item" {
			data, err := os.ReadFile(filepath.Join(manifestPath, f.Name()))
			if err != nil { continue }

			var manifest EpicManifest
			if err := json.Unmarshal(data, &manifest); err != nil { continue }

			games = append(games, models.LibraryGame{
				ID:       manifest.AppName,
				Name:     manifest.DisplayName,
				Platform: "Epic",
				// У Epic нет публичного API картинок по ID, используем заглушку или локальный файл если есть
				IconURL:  "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:  manifest.InstallLocation,
				// Epic не хранит локальную статистику по аккаунтам в открытом виде так просто, как Steam
				AvailableOnAccounts: []models.AccountStat{}, 
			})
		}
	}
	return games
}

// ScanEpicAccounts - Epic хранит логин в зашифрованном файле, 
// для оффлайн режима мы можем только предположить наличие аккаунтов 
// или найти Config файлы. Для MVP вернем заглушку "Main Account".
func ScanEpicAccounts() []models.Account {
	// В будущем здесь можно парсить %LOCALAPPDATA%\EpicGamesLauncher\Saved\Config\Windows\GameUserSettings.ini
	return []models.Account{
		{
			ID:          "EpicMain",
			DisplayName: "Current User",
			Username:    "EpicUser",
			Platform:    "Epic",
		},
	}
}