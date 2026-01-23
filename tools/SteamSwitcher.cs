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

        string targetUser = args[0];
        string gameId = (args.Length > 1) ? args[1] : null;
        
        // Получаем путь к Steam
        string steamPath = GetSteamPath();
        if (string.IsNullOrEmpty(steamPath))
        {
             Console.WriteLine("Error: Steam path not found in Registry.");
             // Пытаемся угадать стандартный путь, если реестр пуст
             if (Directory.Exists("C:\\Program Files (x86)\\Steam"))
                 steamPath = "C:\\Program Files (x86)\\Steam";
             else
                 return;
        }

        Console.WriteLine("[1/4] Killing Steam...");
        KillSteam();

        Console.WriteLine("[2/4] Patching VDF...");
        PatchVdf(steamPath, targetUser);

        Console.WriteLine("[3/4] Updating Registry...");
        SetRegistry(targetUser);

        Console.WriteLine("[4/4] Starting Steam...");
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
        string[] procs = { "steam", "steamwebhelper", "GameOverlayUI" };
        foreach (string procName in procs)
        {
            foreach (Process proc in Process.GetProcessesByName(procName))
            {
                try { proc.Kill(); } catch { }
            }
        }
        Thread.Sleep(2000); // Ждем 2 секунды
    }

    static void SetRegistry(string username)
    {
        string keyPath = "Software\\Valve\\Steam";
        try
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(keyPath, true))
            {
                if (key != null)
                {
                    key.SetValue("AutoLoginUser", username, RegistryValueKind.String);
                    key.SetValue("RememberPassword", 1, RegistryValueKind.DWord);
                    key.SetValue("SkipOfflineModeWarning", 1, RegistryValueKind.DWord);
                }
            }

            // Очистка ActiveUser
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(keyPath + "\\ActiveProcess", true))
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

    static void PatchVdf(string steamPath, string username)
    {
        string vdfPath = Path.Combine(steamPath, "config", "loginusers.vdf");
        if (!File.Exists(vdfPath)) 
        {
            Console.WriteLine("Warning: VDF file not found at " + vdfPath);
            return;
        }

        try
        {
            string content = File.ReadAllText(vdfPath);

            // 1. Сбрасываем MostRecent
            content = Regex.Replace(content, "\"MostRecent\"\\s+\"1\"", "\"MostRecent\"		\"0\"");

            // 2. Ищем пользователя
            int userIndex = content.IndexOf("\"" + username + "\"", StringComparison.OrdinalIgnoreCase);
            
            if (userIndex != -1)
            {
                // Ищем конец блока
                int blockEnd = content.IndexOf('}', userIndex);
                if (blockEnd != -1)
                {
                    string block = content.Substring(userIndex, blockEnd - userIndex);
                    
                    // Меняем MostRecent на 1
                    string newBlock = Regex.Replace(block, "\"MostRecent\"\\s+\"0\"", "\"MostRecent\"		\"1\"");
                    
                    // Обновляем Timestamp на текущий (Unix время)
                    TimeSpan t = DateTime.UtcNow - new DateTime(1970, 1, 1);
                    int secondsSinceEpoch = (int)t.TotalSeconds;
                    newBlock = Regex.Replace(newBlock, "\"Timestamp\"\\s+\"\\d+\"", "\"Timestamp\"		\"" + secondsSinceEpoch + "\"");

                    content = content.Remove(userIndex, blockEnd - userIndex).Insert(userIndex, newBlock);
                }
            }
            else
            {
                Console.WriteLine("Warning: User " + username + " not found in VDF file. Registry will be used as fallback.");
            }

            File.WriteAllText(vdfPath, content);
        }
        catch (Exception ex)
        {
            Console.WriteLine("File Error: " + ex.Message);
        }
    }

    static void StartSteam(string steamPath, string appId)
    {
        string exe = Path.Combine(steamPath, "steam.exe");
        string args = (appId != null && appId.Length > 0) ? "-applaunch " + appId : "";
        
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = exe,
                Arguments = args,
                UseShellExecute = true
            });
        }
        catch (Exception ex)
        {
            Console.WriteLine("Start Error: " + ex.Message);
        }
    }
}