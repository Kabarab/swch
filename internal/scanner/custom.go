package scanner

import (
	"encoding/json"
	"os"
	"path/filepath"
	"swch/internal/models"
)

// Получаем путь к файлу настроек: %APPDATA%\swch\custom_games.json
func getCustomGamesPath() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch")
	_ = os.MkdirAll(path, 0755)
	return filepath.Join(path, "custom_games.json")
}

// SaveCustomGame сохраняет новую игру в JSON
func SaveCustomGame(game models.LibraryGame) error {
	games := LoadCustomGames()
	games = append(games, game)
	
	data, err := json.MarshalIndent(games, "", "  ")
	if err != nil { return err }
	
	return os.WriteFile(getCustomGamesPath(), data, 0644)
}

// LoadCustomGames загружает список своих игр
func LoadCustomGames() []models.LibraryGame {
	path := getCustomGamesPath()
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return []models.LibraryGame{}
	}

	data, err := os.ReadFile(path)
	if err != nil { return []models.LibraryGame{} }

	var games []models.LibraryGame
	json.Unmarshal(data, &games)
	return games
}