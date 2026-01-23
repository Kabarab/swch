package scanner

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"

	"github.com/andygrunwald/vdf"
)

type SteamScanner struct {
	Path string
}

func NewSteamScanner() *SteamScanner {
	path, _ := sys.GetSteamPath()
	return &SteamScanner{Path: path}
}

// GetGames возвращает список игр со всех библиотек
func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" { return games }

	accounts := s.GetAccounts()
	libraryPaths := s.getLibraryFolders() // <--- Получаем все папки библиотек

	// Перебираем каждую библиотеку (C:\Steam, D:\Games\SteamLibrary и т.д.)
	for _, libPath := range libraryPaths {
		steamAppsPath := filepath.Join(libPath, "steamapps")
		files, err := os.ReadDir(steamAppsPath)
		if err != nil { continue }

		for _, f := range files {
			// Ищем appmanifest_ID.acf
			if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
				data := parseVdf(filepath.Join(steamAppsPath, f.Name()))
				appState, ok := data["AppState"].(map[string]interface{})
				if !ok { continue }

				name, _ := appState["name"].(string)
				appID, _ := appState["appid"].(string)
				installDir, _ := appState["installdir"].(string) // Папка установки

				// Полный путь к exe (приблизительно, так как Steam не хранит путь к exe в манифесте явно)
				// Но для запуска нам нужен только ID, путь для информации
				fullPath := filepath.Join(steamAppsPath, "common", installDir)

				var owners []models.AccountStat
				for _, acc := range accounts {
					if s.ownsGame(acc.ID, appID) {
						owners = append(owners, models.AccountStat{
							AccountID:   acc.ID,
							DisplayName: acc.DisplayName,
							PlaytimeMin: 0, // Можно доработать парсинг времени
						})
					}
				}

				games = append(games, models.LibraryGame{
					ID:                  appID,
					Name:                name,
					Platform:            "Steam",
					IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
					ExePath:             fullPath,
					AvailableOnAccounts: owners,
				})
			}
		}
	}
	return games
}

// getLibraryFolders читает libraryfolders.vdf чтобы найти все диски с играми
func (s *SteamScanner) getLibraryFolders() []string {
	paths := []string{s.Path} // Добавляем основную папку Steam по умолчанию

	vdfPath := filepath.Join(s.Path, "steamapps", "libraryfolders.vdf")
	f, err := os.Open(vdfPath)
	if err != nil { return paths }
	defer f.Close()

	p := vdf.NewParser(f)
	m, err := p.Parse()
	if err != nil { return paths }

	// Структура: "libraryfolders" -> "0", "1", "2" -> "path"
	if libFolders, ok := m["libraryfolders"].(map[string]interface{}); ok {
		for _, v := range libFolders {
			if folderData, ok := v.(map[string]interface{}); ok {
				if path, ok := folderData["path"].(string); ok {
					// Проверяем, нет ли уже этого пути (чтобы не дублировать основную)
					if !strings.EqualFold(path, s.Path) {
						paths = append(paths, path)
					}
				}
			}
		}
	}
	return paths
}

func (s *SteamScanner) GetAccounts() []models.Account {
	var accounts []models.Account
	// Путь к конфигам пользователей всегда в основной папке Steam
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	loginData := parseVdf(loginUsersPath)
	
	userDataPath := filepath.Join(s.Path, "userdata")
	entries, _ := os.ReadDir(userDataPath)

	for _, entry := range entries {
		if !entry.IsDir() { continue }
		steamID3 := entry.Name()
		if steamID3 == "0" || steamID3 == "anonymous" || steamID3 == "ac" { continue }

		displayName := "User " + steamID3
		username := steamID3

		// Конвертация ID3 -> ID64
		id3, _ := strconv.ParseInt(steamID3, 10, 64)
		id64 := id3 + 76561197960265728
		id64Str := strconv.FormatInt(id64, 10)

		// Поиск имен
		if users, ok := loginData["users"].(map[string]interface{}); ok {
			if u, found := users[id64Str].(map[string]interface{}); found {
				if n, ok := u["PersonaName"].(string); ok { displayName = n }
				if a, ok := u["AccountName"].(string); ok { username = a }
			}
		}

		accounts = append(accounts, models.Account{
			ID:          steamID3,
			DisplayName: displayName,
			Username:    username,
			Platform:    "Steam",
		})
	}
	return accounts
}

// ownsGame проверяет localconfig.vdf.
// Это "быстрая" проверка через поиск подстроки, чтобы не парсить огромный файл.
func (s *SteamScanner) ownsGame(steamID3 string, appID string) bool {
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	contentBytes, err := ioutil.ReadFile(localConfigPath)
	if err != nil { return false }
	content := string(contentBytes)
	
	// Ищем ключ "AppID" в кавычках
	return strings.Contains(content, fmt.Sprintf(`"%s"`, appID))
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil { return nil }
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}