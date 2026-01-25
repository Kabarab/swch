package scanner

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strconv"
	"sync"
)

var (
	steamNameCache map[string]string
	cacheMutex     sync.RWMutex
	initOnce       sync.Once
)

type steamAppListResp struct {
	Applist struct {
		Apps []struct {
			AppID int    `json:"appid"`
			Name  string `json:"name"`
		} `json:"apps"`
	} `json:"applist"`
}

const cacheFileName = "steam_names_cache.json"

// EnsureSteamNames загружает имена из кэша или скачивает их из интернета.
// Блокирует выполнение до получения данных (нужно для первого запуска).
func EnsureSteamNames() {
	initOnce.Do(func() {
		// 1. Пытаемся загрузить с диска
		if loadFromDisk() {
			return
		}
		// 2. Если нет кэша, скачиваем (это может занять пару секунд)
		fetchAndSave()
	})
}

func loadFromDisk() bool {
	data, err := os.ReadFile(cacheFileName)
	if err != nil {
		return false
	}

	var cache map[string]string
	if err := json.Unmarshal(data, &cache); err != nil {
		return false
	}

	cacheMutex.Lock()
	steamNameCache = cache
	cacheMutex.Unlock()
	return true
}

func fetchAndSave() {
	fmt.Println("Downloading Steam App List...")
	resp, err := http.Get("https://api.steampowered.com/ISteamApps/GetAppList/v2/")
	if err != nil {
		fmt.Println("Error fetching steam apps:", err)
		return
	}
	defer resp.Body.Close()

	var apiResp steamAppListResp
	if err := json.NewDecoder(resp.Body).Decode(&apiResp); err != nil {
		fmt.Println("Error decoding steam apps:", err)
		return
	}

	newCache := make(map[string]string)
	for _, app := range apiResp.Applist.Apps {
		newCache[strconv.Itoa(app.AppID)] = app.Name
	}

	cacheMutex.Lock()
	steamNameCache = newCache
	cacheMutex.Unlock()

	// Сохраняем на диск
	if data, err := json.Marshal(newCache); err == nil {
		os.WriteFile(cacheFileName, data, 0644)
	}
	fmt.Println("Steam App List saved.")
}

// ResolveSteamName возвращает имя игры по AppID.
// Если имени нет в базе, возвращает пустую строку.
func ResolveSteamName(appID string) string {
	cacheMutex.RLock()
	defer cacheMutex.RUnlock()
	if steamNameCache == nil {
		return ""
	}
	return steamNameCache[appID]
}
