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

        string steamPath = GetSteamPath();
        if (string.IsNullOrEmpty(steamPath))
        {
             if (Directory.Exists("C:\\Program Files (x86)\\Steam"))
                 steamPath = "C:\\Program Files (x86)\\Steam";
             else if (Directory.Exists("C:\\Program Files\\Steam"))
                 steamPath = "C:\\Program Files\\Steam";
             else
             {
                 Console.WriteLine("Error: Steam path not found.");
                 return;
             }
        }

        Console.WriteLine("[1/4] Closing Steam processes...");
        KillSteam();

        Console.WriteLine("[2/4] Patching loginusers.vdf...");
        PatchVdf(steamPath, targetUser);

        Console.WriteLine("[3/4] Setting Registry keys for " + targetUser + "...");
        SetRegistry(targetUser);

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
        string[] procs = { "steam", "steamwebhelper", "GameOverlayUI" };
        int retries = 5;
        
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
            
            Thread.Sleep(1000);
            retries--;
        }
        // Extra wait to ensure file locks are released
        Thread.Sleep(1000);
    }

    static void SetRegistry(string username)
    {
        string keyPath = "Software\\Valve\\Steam";
        try
        {
            // Force create/open main key
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath))
            {
                if (key != null)
                {
                    key.SetValue("AutoLoginUser", username, RegistryValueKind.String);
                    key.SetValue("RememberPassword", 1, RegistryValueKind.DWord);
                    key.SetValue("SkipOfflineModeWarning", 1, RegistryValueKind.DWord);
                }
            }

            // Force create/open ActiveProcess key
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath + "\\ActiveProcess"))
            {
                if (key != null)
                {
                    // Setting this to 0 tells Steam "The last user logged out safely, please use AutoLoginUser"
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

            // 1. Reset "MostRecent" globally
            content = Regex.Replace(content, "\"MostRecent\"\\s+\"1\"", "\"MostRecent\"      \"0\"");

            // 2. Find and update the specific user block
            string userBlockPattern = "(\\\"" + "\\d{17}" + "\\\"\\s*\\{[^{}]*\\\"AccountName\\\"\\s*\\\"" + Regex.Escape(targetUsername) + "\\\"[^{}]*\\})";
            
            content = Regex.Replace(content, userBlockPattern, new MatchEvaluator(delegate(Match match)
            {
                string block = match.Value;
                
                TimeSpan t = DateTime.UtcNow - new DateTime(1970, 1, 1);
                int now = (int)t.TotalSeconds;

                // Helper to replace or insert key-value pair
                block = UpdateOrInsert(block, "MostRecent", "1");
                block = UpdateOrInsert(block, "Timestamp", now.ToString());
                block = UpdateOrInsert(block, "AllowAutoLogin", "1");
                block = UpdateOrInsert(block, "RememberPassword", "1"); // CRITICAL for auto-login
                block = UpdateOrInsert(block, "WantsOfflineMode", "0"); // Ensure we don't start offline

                return block;
            }), RegexOptions.IgnoreCase | RegexOptions.Singleline);

            File.WriteAllText(vdfPath, content);
        }
        catch (Exception ex)
        {
            Console.WriteLine("VDF Patch Error: " + ex.Message);
        }
    }

    // Helper method to safely update VDF string block
    static string UpdateOrInsert(string block, string key, string value)
    {
        string pattern = "\"" + key + "\"\\s+\"\\d+\"";
        if (Regex.IsMatch(block, pattern))
        {
            return Regex.Replace(block, pattern, "\"" + key + "\"      \"" + value + "\"");
        }
        else
        {
            // Insert before the closing brace
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