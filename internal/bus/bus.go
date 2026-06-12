package bus

import (
	"context"
	"sync"

	wruntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

// Emitter — абстракция над Wails events. Сервисы (воркеры) шлют события
// через этот интерфейс, а не через wails runtime напрямую, поэтому
// unit-тесты не требуют запущенного UI (см. PRD, AI-Driven QA).
type Emitter interface {
	Emit(event string, data ...interface{})
}

// WailsEmitter доставляет события на фронтенд через wails runtime.
// Контекст Wails появляется только в App.Startup, поэтому он выставляется
// отдельно через SetContext; до этого момента события молча отбрасываются.
type WailsEmitter struct {
	mu  sync.RWMutex
	ctx context.Context
}

func NewWailsEmitter() *WailsEmitter {
	return &WailsEmitter{}
}

func (e *WailsEmitter) SetContext(ctx context.Context) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.ctx = ctx
}

func (e *WailsEmitter) Emit(event string, data ...interface{}) {
	e.mu.RLock()
	ctx := e.ctx
	e.mu.RUnlock()
	if ctx == nil {
		return
	}
	wruntime.EventsEmit(ctx, event, data...)
}

// NullEmitter отбрасывает все события (для тестов и headless-запусков).
type NullEmitter struct{}

func (NullEmitter) Emit(event string, data ...interface{}) {}
