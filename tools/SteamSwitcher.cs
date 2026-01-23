using Microsoft.Win32;
using System;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;
using System.Threading;

public class Program
{
    public static void Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.WriteLine("Error: No username provided.");
            return;
        }

        string targetUser = args[0]; // Важно: это должен быть логин для входа!
        string gameId = (args.Length > 1) ? args[1] : null;

        string steamPath = GetSteamPath();
        if (string.IsNullOrEmpty(steamPath))
        {
             if (Directory.Exists("C:\\Program Files (x86)\\Steam")) steamPath = "C:\\Program Files (x86)\\Steam";
             else if (Directory.Exists("C:\\Program Files\\Steam")) steamPath = "C:\\Program Files\\Steam";
             else { Console.WriteLine("Error: Steam path not found."); return; }
        }

        Console.WriteLine("--- SWITCHING STEAM ACCOUNT ---");
        Console.WriteLine("Target: " + targetUser);

        // 1. Полное закрытие Steam
        Console.WriteLine("[1/4] Stopping Steam processes...");
        KillSteam();

        // 2. Настройка файла конфигурации (интерфейс)
        Console.WriteLine("[2/4] Patching loginusers.vdf...");
        PatchVdf(steamPath, targetUser);

        // 3. Настройка реестра (авто-вход и снятие галочки "Спрашивать аккаунт")
        Console.WriteLine("[3/4] Configuring Registry...");
        SetRegistry(targetUser);

        // 4. Запуск
        Console.WriteLine("[4/4] Launching Steam...");
        StartSteam(steamPath, gameId);
    }

    static string GetSteamPath()
    {
        try 
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey("Software\\Valve\\Steam"))
            {
                if (key != null)
                {
                    object val = key.GetValue("SteamPath");
                    if (val != null) return val.ToString().Replace("/", "\\");
                }
            }
        }
        catch {}
        return null;
    }

    static void KillSteam()
    {
        // Убиваем все процессы, связанные со Steam, чтобы освободить файлы и реестр
        string[] procs = { "steam", "steamwebhelper", "GameOverlayUI", "steamservice" };
        int retries = 20; // Пытаемся закрыть в течение 20 циклов (около 5-6 секунд)
        
        while (retries > 0)
        {
            bool anyAlive = false;
            foreach (string procName in procs)
            {
                Process[] running = Process.GetProcessesByName(procName);
                if (running.Length > 0)
                {
                    anyAlive = true;
                    foreach (Process proc in running)
                    {
                        try { proc.Kill(); } catch { }
                    }
                }
            }
            if (!anyAlive) break;
            Thread.Sleep(300);
            retries--;
        }
        Thread.Sleep(1000); // Контрольная пауза
    }

    static void SetRegistry(string username)
    {
        string keyPath = "Software\\Valve\\Steam";
        try
        {
            // Создаем или открываем ключ (CreateSubKey работает надежнее OpenSubKey для записи)
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath))
            {
                if (key != null)
                {
                    // ЭТИ ПАРАМЕТРЫ СНИМАЮТ ГАЛОЧКУ "СПРАШИВАТЬ АККАУНТ":
                    key.SetValue("AutoLoginUser", username, RegistryValueKind.String);
                    key.SetValue("RememberPassword", 1, RegistryValueKind.DWord);
                    
                    // Дополнительные параметры для тихого запуска
                    key.SetValue("SkipOfflineModeWarning", 1, RegistryValueKind.DWord);
                    key.SetValue("Language", "russian", RegistryValueKind.String); // Можно убрать, если не нужно
                }
            }

            // Сбрасываем "Активного пользователя", чтобы Steam перечитал AutoLoginUser
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath + "\\ActiveProcess"))
            {
                if (key != null)
                {
                    key.SetValue("ActiveUser", 0, RegistryValueKind.DWord);
                }
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine("Registry Error: " + ex.Message);
        }
    }

    static void PatchVdf(string steamPath, string targetUsername)
    {
        string vdfPath = Path.Combine(steamPath, "config", "loginusers.vdf");
        if (!File.Exists(vdfPath)) return;

        try
        {
            string content = File.ReadAllText(vdfPath);

            // 1. Глобально убираем флаг MostRecent у ВСЕХ пользователей
            // Это гарантирует, что окно выбора аккаунта не появится из-за файла
            content = Regex.Replace(content, "\"MostRecent\"\\s+\"1\"", "\"MostRecent\"      \"0\"");

            // 2. Ищем блок нужного пользователя и ставим ему правильные флаги
            // Используем Regex.Escape, чтобы спецсимволы в логине не ломали поиск
            string userBlockPattern = "(\\\"" + "\\d{17}" + "\\\"\\s*\\{[^{}]*\\\"AccountName\\\"\\s*\\\"" + Regex.Escape(targetUsername) + "\\\"[^{}]*\\})";
            
            content = Regex.Replace(content, userBlockPattern, new MatchEvaluator(delegate(Match match)
            {
                string block = match.Value;
                
                TimeSpan t = DateTime.UtcNow - new DateTime(1970, 1, 1);
                int now = (int)t.TotalSeconds;

                // Функция EnsureKey проверяет наличие ключа: если есть - меняет, если нет - добавляет
                block = EnsureKey(block, "MostRecent", "1");
                block = EnsureKey(block, "Timestamp", now.ToString());
                block = EnsureKey(block, "AllowAutoLogin", "1");
                block = EnsureKey(block, "RememberPassword", "1"); 
                block = EnsureKey(block, "WantsOfflineMode", "0");

                return block;
            }), RegexOptions.IgnoreCase | RegexOptions.Singleline);

            File.WriteAllText(vdfPath, content);
        }
        catch (Exception ex)
        {
            Console.WriteLine("VDF Patch Error: " + ex.Message);
        }
    }

    static string EnsureKey(string block, string key, string value)
    {
        string pattern = "\"" + key + "\"\\s+\"\\d+\"";
        if (Regex.IsMatch(block, pattern))
        {
            // Замена существующего значения
            return Regex.Replace(block, pattern, "\"" + key + "\"      \"" + value + "\"");
        }
        else
        {
            // Добавление нового ключа перед закрывающей скобкой
            int lastBrace = block.LastIndexOf('}');
            if (lastBrace != -1)
            {
                return block.Insert(lastBrace, "\t\"" + key + "\"      \"" + value + "\"\n\t");
            }
        }
        return block;
    }

    static void StartSteam(string steamPath, string appId)
    {
        string exe = Path.Combine(steamPath, "steam.exe");
        string args = (string.IsNullOrEmpty(appId)) ? "" : "-applaunch " + appId;
        
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = exe,
                Arguments = args,
                UseShellExecute = true,
                WorkingDirectory = steamPath
            });
        }
        catch (Exception ex)
        {
            Console.WriteLine("Start Error: " + ex.Message);
        }
    }
}