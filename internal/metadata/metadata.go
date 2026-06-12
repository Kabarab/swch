// Package metadata загружает и кэширует метаданные игр (описание, жанры,
// дата выхода, разработчики) — аналог metadata downloader из Playnite.
// Источник: Steam Store API (бесплатный, без API-ключа).
package metadata

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"
)

const cacheFileName = "metadata_cache.json"

type Meta struct {
	Description string   `json:"description"`
	Genres      []string `json:"genres"`
	ReleaseDate string   `json:"releaseDate"`
	Developers  []string `json:"developers"`
	FetchedAt   int64    `json:"fetchedAt"`
}

type Store struct {
	mu    sync.RWMutex
	cache map[string]Meta // ключ — ID игры
	path  string
}

func NewStore() *Store {
	configDir, _ := os.UserConfigDir()
	dir := filepath.Join(configDir, "swch")
	_ = os.MkdirAll(dir, 0755)

	s := &Store{
		cache: make(map[string]Meta),
		path:  filepath.Join(dir, cacheFileName),
	}
	s.load()
	return s
}

func (s *Store) load() {
	data, err := os.ReadFile(s.path)
	if err != nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	_ = json.Unmarshal(data, &s.cache)
	if s.cache == nil {
		s.cache = make(map[string]Meta)
	}
}

func (s *Store) save() error {
	s.mu.RLock()
	data, err := json.MarshalIndent(s.cache, "", "  ")
	s.mu.RUnlock()
	if err != nil {
		return fmt.Errorf("failed to marshal metadata cache: %w", err)
	}
	if err := os.WriteFile(s.path, data, 0644); err != nil {
		return fmt.Errorf("failed to write metadata cache: %w", err)
	}
	return nil
}

func (s *Store) Get(gameID string) (Meta, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	m, ok := s.cache[gameID]
	return m, ok
}

func (s *Store) Put(gameID string, m Meta) {
	s.mu.Lock()
	s.cache[gameID] = m
	s.mu.Unlock()
	if err := s.save(); err != nil {
		fmt.Printf("[Metadata] Ошибка сохранения кэша: %v\n", err)
	}
}

// --- Steam Store API ---

type steamDetailsResp struct {
	Success bool `json:"success"`
	Data    struct {
		ShortDescription string `json:"short_description"`
		Genres           []struct {
			Description string `json:"description"`
		} `json:"genres"`
		ReleaseDate struct {
			Date string `json:"date"`
		} `json:"release_date"`
		Developers []string `json:"developers"`
	} `json:"data"`
}

// FetchSteam запрашивает метаданные игры из Steam Store API по appID.
func FetchSteam(appID string) (Meta, error) {
	url := fmt.Sprintf("https://store.steampowered.com/api/appdetails?appids=%s&l=russian", appID)
	client := &http.Client{Timeout: 10 * time.Second}

	resp, err := client.Get(url)
	if err != nil {
		return Meta{}, fmt.Errorf("steam store request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return Meta{}, fmt.Errorf("steam store returned status %d", resp.StatusCode)
	}

	var parsed map[string]steamDetailsResp
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return Meta{}, fmt.Errorf("failed to decode steam store response: %w", err)
	}

	entry, ok := parsed[appID]
	if !ok || !entry.Success {
		return Meta{}, fmt.Errorf("steam store has no data for app %s", appID)
	}

	m := Meta{
		Description: entry.Data.ShortDescription,
		ReleaseDate: entry.Data.ReleaseDate.Date,
		Developers:  entry.Data.Developers,
		FetchedAt:   time.Now().Unix(),
	}
	for _, g := range entry.Data.Genres {
		if g.Description != "" {
			m.Genres = append(m.Genres, g.Description)
		}
	}
	return m, nil
}
