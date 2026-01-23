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

// Константа для конвертации SteamID3 -> SteamID64
const steamID64Identifier = 76561197960265728

func (s *SteamScanner) Scan() []models.Account {
	var accounts []models.Account
	if s.Path == "" {
		return accounts
	}

	// 1. Ищем установленные игры
	installedGames := s.scanInstalledGames()

	// 2. Читаем файл с пользователями (loginusers.vdf)
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	loginData := parseVdf(loginUsersPath) // Возвращает map[string]interface{} где ключи - SteamID64

	// 3. Идем по папкам userdata (названия папок - это SteamID3)
	userDataPath := filepath.Join(s.Path, "userdata")
	entries, _ := os.ReadDir(userDataPath)

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		steamID3Str := entry.Name()
		
		// Пропускаем системные папки
		if steamID3Str == "0" || steamID3Str == "anonymous" || steamID3Str == "ac" {
			continue
		}

		// --- ЛОГИКА ОПРЕДЕЛЕНИЯ ИМЕН ---
		
		// По умолчанию
		displayName := "User " + steamID3Str
		username := ""

		// Пытаемся конвертировать ID3 (папка) в ID64 (ключ в файле)
		// ID64 = ID3 + 76561197960265728
		id3, err := strconv.ParseInt(steamID3Str, 10, 64)
		if err == nil {
			id64 := id3 + steamID64Identifier
			id64Str := strconv.FormatInt(id64, 10)

			// Ищем этот ID64 в данных loginusers.vdf
			// Структура VDF: "users" -> { "7656..." : { "AccountName": "...", "PersonaName": "..." } }
			if users, ok := loginData["users"].(map[string]interface{}); ok {
				if userData, found := users[id64Str].(map[string]interface{}); found {
					// Нашли! Берем данные
					if pName, ok := userData["PersonaName"].(string); ok {
						displayName = pName
					}
					if accName, ok := userData["AccountName"].(string); ok {
						username = accName
					}
				}
			} else {
				// Если структура файла плоская (иногда бывает без "users")
				if userData, found := loginData[id64Str].(map[string]interface{}); found {
					if pName, ok := userData["PersonaName"].(string); ok {
						displayName = pName
					}
					if accName, ok := userData["AccountName"].(string); ok {
						username = accName
					}
				}
			}
		}

		// Если логин так и не нашли, используем ID как заглушку
		if username == "" {
			username = steamID3Str
		}

		// 4. Фильтруем игры
		myGames := s.filterGamesForAccount(steamID3Str, installedGames)

		if len(myGames) > 0 {
			acc := models.Account{
				ID:          steamID3Str,
				DisplayName: displayName, // Никнейм (NazarSlayer)
				Username:    username,    // Логин (nazarn2008)
				Platform:    "Steam",
				OwnedGames:  myGames,
			}
			accounts = append(accounts, acc)
		}
	}
	return accounts
}

func (s *SteamScanner) scanInstalledGames() map[string]string {
	games := make(map[string]string)
	steamAppsPath := filepath.Join(s.Path, "steamapps")
	
	files, err := os.ReadDir(steamAppsPath)
	if err != nil { return games }

	for _, f := range files {
		if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
			fullPath := filepath.Join(steamAppsPath, f.Name())
			data := parseVdf(fullPath)
			
			if appState, ok := data["AppState"].(map[string]interface{}); ok {
				name, _ := appState["name"].(string)
				appid, _ := appState["appid"].(string)
				if appid != "" {
					games[appid] = name
				}
			}
		}
	}
	return games
}

func (s *SteamScanner) filterGamesForAccount(steamID3 string, installedGames map[string]string) []models.Game {
	var myGames []models.Game
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	
	contentBytes, err := ioutil.ReadFile(localConfigPath)
	if err != nil { return myGames }
	content := string(contentBytes)

	for appID, name := range installedGames {
		if strings.Contains(content, fmt.Sprintf(`"%s"`, appID)) {
			myGames = append(myGames, models.Game{
				ID:       appID,
				Name:     name,
				Platform: "Steam",
				ImageURL: fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
			})
		}
	}
	return myGames
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil { return nil }
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}