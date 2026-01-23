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

        string targetUser = args[0]; // ВАЖНО: Это должен быть ЛОГИН (AccountName), а не никнейм!
        string gameId = (args.Length > 1) ? args[1] : null;

        string steamPath = GetSteamPath();
        if (string.IsNullOrEmpty(steamPath))
        {
             // Стандартные пути, если реестр пуст
             if (Directory.Exists("C:\\Program Files (x86)\\Steam")) steamPath = "C:\\Program Files (x86)\\Steam";
             else if (Directory.Exists("C:\\Program Files\\Steam")) steamPath = "C:\\Program Files\\Steam";
             else { Console.WriteLine("Error: Steam path not found."); return; }
        }

        Console.WriteLine("--- STEAM ACCOUNT SWITCHER ---");
        Console.WriteLine("Target Login: " + targetUser);

        // 1. Закрываем Steam (Критически важно!)
        Console.WriteLine("[1/4] Killing Steam processes...");
        KillSteam();

        // 2. Обновляем файл loginusers.vdf (для интерфейса)
        Console.WriteLine("[2/4] Patching loginusers.vdf...");
        PatchVdf(steamPath, targetUser);

        // 3. Обновляем Реестр (для авто-входа)
        Console.WriteLine("[3/4] Updating Registry for AutoLogin...");
        SetRegistry(targetUser);

        // 4. Запускаем Steam
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
        string[] procs = { "steam", "steamwebhelper", "GameOverlayUI", "steamservice" };
        int maxRetries = 10; // Ждем до 10 секунд
        
        for (int i = 0; i < maxRetries; i++)
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
                        try { proc.Kill(); } catch { } // Пытаемся убить
                    }
                }
            }

            if (!anyAlive) return; // Если все мертвы — выходим
            Thread.Sleep(1000); // Ждем секунду
        }
    }

    static void SetRegistry(string username)
    {
        string keyPath = "Software\\Valve\\Steam";
        try
        {
            // 1. Основные ключи в HKCU
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath))
            {
                if (key != null)
                {
                    // ЭТО САМОЕ ГЛАВНОЕ ДЛЯ АВТО-ВХОДА:
                    key.SetValue("AutoLoginUser", username, RegistryValueKind.String);
                    key.SetValue("RememberPassword", 1, RegistryValueKind.DWord);
                    key.SetValue("SkipOfflineModeWarning", 1, RegistryValueKind.DWord);
                }
            }

            // 2. Сброс активного пользователя (заставляет Steam перечитать AutoLoginUser)
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

            // Сбрасываем флаг MostRecent у всех
            content = Regex.Replace(content, "\"MostRecent\"\\s+\"1\"", "\"MostRecent\"      \"0\"");

            // Ищем блок конкретного пользователя по AccountName
            // Внимание: targetUsername должен быть ЛОГИНОМ
            string userBlockPattern = "(\\\"" + "\\d{17}" + "\\\"\\s*\\{[^{}]*\\\"AccountName\\\"\\s*\\\"" + Regex.Escape(targetUsername) + "\\\"[^{}]*\\})";
            
            content = Regex.Replace(content, userBlockPattern, new MatchEvaluator(delegate(Match match)
            {
                string block = match.Value;
                
                TimeSpan t = DateTime.UtcNow - new DateTime(1970, 1, 1);
                int now = (int)t.TotalSeconds;

                // Функция для замены или добавления параметра
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
        // Если ключ уже есть — заменяем значение
        string pattern = "\"" + key + "\"\\s+\"\\d+\"";
        if (Regex.IsMatch(block, pattern))
        {
            return Regex.Replace(block, pattern, "\"" + key + "\"      \"" + value + "\"");
        }
        else
        {
            // Если ключа нет — вставляем его перед закрывающей скобкой }
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