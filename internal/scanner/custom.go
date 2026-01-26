package scanner

import (
	"encoding/json"
	"os"
	"path/filepath"
	"swch/internal/models"
)

func getCustomGamesPath() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch")
	_ = os.MkdirAll(path, 0755)
	return filepath.Join(path, "custom_games.json")
}

// Вспомогательная функция для сохранения списка
func saveGamesList(games []models.LibraryGame) error {
	data, err := json.MarshalIndent(games, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(getCustomGamesPath(), data, 0644)
}

func SaveCustomGame(game models.LibraryGame) error {
	games := LoadCustomGames()
	games = append(games, game)
	return saveGamesList(games)
}

func LoadCustomGames() []models.LibraryGame {
	path := getCustomGamesPath()
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return []models.LibraryGame{}
	}

	data, err := os.ReadFile(path)
	if err != nil {
		return []models.LibraryGame{}
	}

	var games []models.LibraryGame
	json.Unmarshal(data, &games)
	return games
}

// Новая функция: Удаление игры
func RemoveCustomGame(gameID string) error {
	games := LoadCustomGames()
	var newGames []models.LibraryGame
	for _, g := range games {
		if g.ID != gameID {
			newGames = append(newGames, g)
		}
	}
	return saveGamesList(newGames)
}

// Новая функция: Обновление картинки
func UpdateCustomGameIcon(gameID, iconPath string) error {
	games := LoadCustomGames()
	for i := range games {
		if games[i].ID == gameID {
			games[i].IconURL = iconPath
			break
		}
	}
	return saveGamesList(games)
}
