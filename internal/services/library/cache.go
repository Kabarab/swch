package library

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"swch/internal/models"
)

// Cache — персистентный кэш агрегированной библиотеки: UI читает мгновенно,
// фоновый Refresh обновляет. Формат — JSON-файл: текущие объемы (сотни игр)
// не оправдывают SQLite; при росте библиотеки формат меняется только здесь.
type Cache struct {
	path string // пустой путь = кэш только в памяти (тесты, ошибки UserConfigDir)
	mu   sync.RWMutex
	data cacheFile
}

type cacheFile struct {
	UpdatedAt time.Time            `json:"updatedAt"`
	Games     []models.LibraryGame `json:"games"`
}

func NewCache(path string) *Cache {
	c := &Cache{path: path}
	c.load()
	return c
}

func (c *Cache) load() {
	if c.path == "" {
		return
	}
	data, err := os.ReadFile(c.path)
	if err != nil {
		return // кэша еще нет — нормальная ситуация первого запуска
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := json.Unmarshal(data, &c.data); err != nil {
		fmt.Printf("[Library] Warning: corrupted cache file %s: %v\n", c.path, err)
		c.data = cacheFile{}
	}
}

// Get возвращает копию записей: app.GetLibrary декорирует их по месту
// (заметки, скрытие, playtime), и кэш не должен этим загрязняться.
func (c *Cache) Get() []models.LibraryGame {
	c.mu.RLock()
	defer c.mu.RUnlock()
	out := make([]models.LibraryGame, len(c.data.Games))
	for i, g := range c.data.Games {
		g.AvailableOnAccounts = append([]models.AccountStat(nil), g.AvailableOnAccounts...)
		out[i] = g
	}
	return out
}

func (c *Cache) Set(games []models.LibraryGame) {
	c.mu.Lock()
	c.data = cacheFile{UpdatedAt: time.Now(), Games: games}
	c.mu.Unlock()
	c.persist()
}

func (c *Cache) persist() {
	if c.path == "" {
		return
	}
	c.mu.RLock()
	data, err := json.MarshalIndent(c.data, "", "  ")
	c.mu.RUnlock()
	if err != nil {
		return
	}
	if err := os.MkdirAll(filepath.Dir(c.path), 0755); err != nil {
		fmt.Printf("[Library] Warning: cannot create cache dir: %v\n", err)
		return
	}
	// Атомарная запись (tmp + rename): падение процесса не оставит битый кэш
	tmp := c.path + ".tmp"
	if err := os.WriteFile(tmp, data, 0644); err != nil {
		fmt.Printf("[Library] Warning: cannot write cache: %v\n", err)
		return
	}
	if err := os.Rename(tmp, c.path); err != nil {
		fmt.Printf("[Library] Warning: cannot replace cache: %v\n", err)
	}
}

func (c *Cache) IsEmpty() bool {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return len(c.data.Games) == 0
}

func (c *Cache) UpdatedAt() time.Time {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.data.UpdatedAt
}
