package epic

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"swch/internal/sys"
)

// Manifest represents a local .item file from Epic Games Launcher
type Manifest struct {
	FormatVersion   int    `json:"FormatVersion"`
	AppName         string `json:"AppName"`
	DisplayName     string `json:"DisplayName"`
	InstallLocation string `json:"InstallLocation"`
	MainGameAppName string `json:"MainGameAppName"`
	CatalogItemID   string `json:"CatalogItemId"`
	NamespaceId     string `json:"NamespaceId"`
}

// InstalledGame represents a game found locally on disk
type InstalledGame struct {
	AppID       string
	Name        string
	InstallPath string
	IsInstalled bool
	Manifest    *Manifest
}

// ScanInstalledGames scans the Epic Games Launcher manifest directory
// to find games that are currently installed/downloaded.
func ScanInstalledGames() (map[string]InstalledGame, error) {
	manifestDir := sys.GetEpicManifestsDir()
	results := make(map[string]InstalledGame)

	entries, err := os.ReadDir(manifestDir)
	if err != nil {
		// It's possible the directory doesn't exist if Epic Launcher isn't installed
		return results, nil
	}

	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".item") {
			path := filepath.Join(manifestDir, entry.Name())

			data, err := os.ReadFile(path)
			if err != nil {
				continue
			}

			var m Manifest
			if err := json.Unmarshal(data, &m); err != nil {
				continue
			}

			// Some .item files are for engine components or internal tools.
			// Usually checking for InstallLocation or AppName is enough.
			if m.AppName == "" || m.InstallLocation == "" {
				continue
			}

			results[m.AppName] = InstalledGame{
				AppID:       m.AppName,
				Name:        m.DisplayName,
				InstallPath: m.InstallLocation,
				IsInstalled: true,
				Manifest:    &m,
			}
		}
	}

	return results, nil
}
