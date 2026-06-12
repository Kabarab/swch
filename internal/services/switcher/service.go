// Package switcher — воркер быстрого переключения аккаунтов (концепция TcNo).
//
// Полный цикл Hot-Swap: завершение процессов лаунчера -> (опционально) подмена
// файлов сессии из снапшота -> активация аккаунта в конфигах -> запуск лаунчера.
// Платформенную специфику предоставляют провайдеры через capability-интерфейсы
// из internal/interfaces; прогресс шагов уходит на фронтенд событиями через
// internal/bus.
package switcher

import (
	"fmt"
	"strings"
	"sync"
	"time"

	"swch/internal/bus"
	"swch/internal/interfaces"
)

// Шаги цикла переключения (поле Status.Step).
const (
	StepKill     = "kill"
	StepRestore  = "restore"
	StepActivate = "activate"
	StepStart    = "start"
	StepDone     = "done"
	StepError    = "error"
)

// Status — payload событий bus.EventSwitcherStatus / bus.EventSwitcherDone.
type Status struct {
	Platform string `json:"platform"`
	Account  string `json:"account"`
	Step     string `json:"step"`
	Message  string `json:"message"`
	OK       bool   `json:"ok"`
}

type Service struct {
	providers map[string]interfaces.PlatformProvider
	emitter   bus.Emitter
	mu        sync.Mutex    // одно переключение за раз: параллельный swap файлов опасен
	killDelay time.Duration // пауза после kill: даем ОС освободить блокировки файлов
}

func NewService(providers map[string]interfaces.PlatformProvider, emitter bus.Emitter) *Service {
	if emitter == nil {
		emitter = bus.NullEmitter{}
	}
	return &Service{
		providers: providers,
		emitter:   emitter,
		killDelay: time.Second,
	}
}

// ValidateAccountName отклоняет имена, способные выйти за пределы каталога
// снапшотов (имя аккаунта становится именем папки на диске).
func ValidateAccountName(name string) error {
	if strings.TrimSpace(name) == "" {
		return fmt.Errorf("account name is empty")
	}
	if strings.ContainsAny(name, `/\`) || name == "." || name == ".." {
		return fmt.Errorf("invalid account name: %q", name)
	}
	return nil
}

// Switch — быстрое переключение без подмены файлов: kill -> activate -> start.
// Для провайдеров без гранулярных способностей откатывается на legacy
// SwitchAccount (провайдер сам управляет процессом внутри).
func (s *Service) Switch(platform, accountName string) error {
	p, err := s.provider(platform)
	if err != nil {
		return err
	}
	if err := ValidateAccountName(accountName); err != nil {
		return err
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	activator, canActivate := p.(interfaces.AccountActivator)
	if !canActivate {
		s.emitStep(StepActivate, platform, accountName, "Switching account via provider...")
		if err := p.SwitchAccount(accountName); err != nil {
			return s.fail(platform, accountName, fmt.Errorf("failed to switch %s account: %w", platform, err))
		}
		return s.done(platform, accountName, "Switched to "+accountName)
	}

	if err := s.killLauncher(p, platform, accountName); err != nil {
		return s.fail(platform, accountName, err)
	}

	s.emitStep(StepActivate, platform, accountName, "Activating account in launcher configs...")
	if err := activator.ActivateAccount(accountName); err != nil {
		return s.fail(platform, accountName, fmt.Errorf("failed to activate account: %w", err))
	}

	if err := s.startLauncher(p, platform, accountName); err != nil {
		return s.fail(platform, accountName, err)
	}
	return s.done(platform, accountName, "Switched to "+accountName)
}

// RestoreSession — полный TcNo-цикл с подменой файлов:
// kill -> restore снапшота -> activate -> start.
func (s *Service) RestoreSession(platform, accountName string) error {
	p, err := s.provider(platform)
	if err != nil {
		return err
	}
	if err := ValidateAccountName(accountName); err != nil {
		return err
	}
	snap, ok := p.(interfaces.SessionSnapshotter)
	if !ok {
		return fmt.Errorf("session snapshots are not supported for %s", platform)
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.killLauncher(p, platform, accountName); err != nil {
		return s.fail(platform, accountName, err)
	}

	s.emitStep(StepRestore, platform, accountName, "Restoring session files from snapshot...")
	if err := snap.RestoreSession(accountName); err != nil {
		return s.fail(platform, accountName, fmt.Errorf("failed to restore session files: %w", err))
	}

	if activator, ok := p.(interfaces.AccountActivator); ok {
		s.emitStep(StepActivate, platform, accountName, "Activating account in launcher configs...")
		if err := activator.ActivateAccount(accountName); err != nil {
			return s.fail(platform, accountName, fmt.Errorf("failed to activate account: %w", err))
		}
	}

	if err := s.startLauncher(p, platform, accountName); err != nil {
		return s.fail(platform, accountName, err)
	}
	return s.done(platform, accountName, "Session restored for "+accountName)
}

// SaveCurrentSession сохраняет файлы текущей сессии лаунчера как снапшот
// аккаунта. Лаунчер останавливать не требуется: файлы только читаются.
func (s *Service) SaveCurrentSession(platform, accountName string) error {
	snap, err := s.snapshotter(platform)
	if err != nil {
		return err
	}
	if err := ValidateAccountName(accountName); err != nil {
		return err
	}
	if err := snap.SaveSession(accountName); err != nil {
		return fmt.Errorf("failed to save %s session: %w", platform, err)
	}
	return nil
}

func (s *Service) ListSessions(platform string) ([]string, error) {
	snap, err := s.snapshotter(platform)
	if err != nil {
		return nil, err
	}
	return snap.ListSessions()
}

func (s *Service) DeleteSession(platform, accountName string) error {
	snap, err := s.snapshotter(platform)
	if err != nil {
		return err
	}
	if err := ValidateAccountName(accountName); err != nil {
		return err
	}
	return snap.DeleteSession(accountName)
}

func (s *Service) provider(platform string) (interfaces.PlatformProvider, error) {
	p, ok := s.providers[platform]
	if !ok {
		return nil, fmt.Errorf("platform not supported: %s", platform)
	}
	return p, nil
}

func (s *Service) snapshotter(platform string) (interfaces.SessionSnapshotter, error) {
	p, err := s.provider(platform)
	if err != nil {
		return nil, err
	}
	snap, ok := p.(interfaces.SessionSnapshotter)
	if !ok {
		return nil, fmt.Errorf("session snapshots are not supported for %s", platform)
	}
	return snap, nil
}

func (s *Service) killLauncher(p interfaces.PlatformProvider, platform, account string) error {
	pc, ok := p.(interfaces.ProcessController)
	if !ok {
		return nil // провайдер не управляет процессом — пропускаем шаг
	}
	s.emitStep(StepKill, platform, account, "Closing launcher...")
	if err := pc.KillLauncher(); err != nil {
		return fmt.Errorf("failed to kill %s launcher: %w", platform, err)
	}
	time.Sleep(s.killDelay)
	return nil
}

func (s *Service) startLauncher(p interfaces.PlatformProvider, platform, account string) error {
	pc, ok := p.(interfaces.ProcessController)
	if !ok {
		return nil
	}
	s.emitStep(StepStart, platform, account, "Starting launcher...")
	if err := pc.StartLauncher(); err != nil {
		return fmt.Errorf("failed to start %s launcher: %w", platform, err)
	}
	return nil
}

func (s *Service) emitStep(step, platform, account, message string) {
	fmt.Printf("[Switcher] %s/%s: %s\n", platform, account, message)
	s.emitter.Emit(bus.EventSwitcherStatus, Status{
		Platform: platform,
		Account:  account,
		Step:     step,
		Message:  message,
		OK:       true,
	})
}

func (s *Service) done(platform, account, message string) error {
	fmt.Printf("[Switcher] %s/%s: %s\n", platform, account, message)
	s.emitter.Emit(bus.EventSwitcherDone, Status{
		Platform: platform,
		Account:  account,
		Step:     StepDone,
		Message:  message,
		OK:       true,
	})
	return nil
}

func (s *Service) fail(platform, account string, err error) error {
	fmt.Printf("[Switcher] %s/%s: ERROR: %v\n", platform, account, err)
	s.emitter.Emit(bus.EventSwitcherDone, Status{
		Platform: platform,
		Account:  account,
		Step:     StepError,
		Message:  err.Error(),
		OK:       false,
	})
	return err
}
