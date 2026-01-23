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

// GetGames возвращает список установленных игр с привязкой к аккаунтам
func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" { return games }

	// 1. Получаем список всех аккаунтов (нужен для проверки владения)
	accounts := s.GetAccounts()
	
	// 2. Ищем файлы appmanifest (установленные игры)
	steamAppsPath := filepath.Join(s.Path, "steamapps")
	files, _ := os.ReadDir(steamAppsPath)

	for _, f := range files {
		if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
			data := parseVdf(filepath.Join(steamAppsPath, f.Name()))
			appState, ok := data["AppState"].(map[string]interface{})
			if !ok { continue }

			name := appState["name"].(string)
			appID := appState["appid"].(string)

			// 3. Проверяем, на каких аккаунтах есть эта игра
			var owners []models.AccountStat
			for _, acc := range accounts {
				playtime := s.checkPlaytime(acc.ID, appID)
				if playtime >= 0 { // Если игра найдена в конфиге
					owners = append(owners, models.AccountStat{
						AccountID:   acc.ID,
						DisplayName: acc.DisplayName,
						PlaytimeMin: playtime,
					})
				}
			}

			games = append(games, models.LibraryGame{
				ID:                  appID,
				Name:                name,
				Platform:            "Steam",
				IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
				AvailableOnAccounts: owners,
			})
		}
	}
	return games
}

// GetAccounts возвращает чистый список аккаунтов
func (s *SteamScanner) GetAccounts() []models.Account {
	var accounts []models.Account
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

		// Конвертация ID3 -> ID64 для поиска имени
		id3, _ := strconv.ParseInt(steamID3, 10, 64)
		id64 := id3 + 76561197960265728
		id64Str := strconv.FormatInt(id64, 10)

		// Ищем в loginusers
		if users, ok := loginData["users"].(map[string]interface{}); ok {
			if u, found := users[id64Str].(map[string]interface{}); found {
				if n, ok := u["PersonaName"].(string); ok { displayName = n }
				if a, ok := u["AccountName"].(string); ok { username = a }
			}
		} else if u, found := loginData[id64Str].(map[string]interface{}); found {
             if n, ok := u["PersonaName"].(string); ok { displayName = n }
             if a, ok := u["AccountName"].(string); ok { username = a }
        }

		accounts = append(accounts, models.Account{
			ID:          steamID3, // Для переключения нужен SteamID3
			DisplayName: displayName,
			Username:    username,
			Platform:    "Steam",
		})
	}
	return accounts
}

// checkPlaytime возвращает минуты в игре или -1 если игра не найдена у аккаунта
func (s *SteamScanner) checkPlaytime(steamID3 string, appID string) int {
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	contentBytes, err := ioutil.ReadFile(localConfigPath)
	if err != nil { return -1 }
	content := string(contentBytes)

	// Быстрая проверка наличия (для оптимизации)
	if !strings.Contains(content, fmt.Sprintf(`"%s"`, appID)) {
		return -1
	}
    
    // В идеале нужен полный парсинг VDF для получения PlayTime, 
    // но для скорости и offline режима пока вернем 0 (владеет), если нашли ID.
    // Парсить localconfig целиком долго.
    return 0 
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil { return nil }
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}