// Deliberately no sockets, network, OnGROW config, trust keys or runtime dependency.
// Replacements are controlled by the isolated lifecycle fixture builder.
using System;
using System.Reflection;
using System.ServiceProcess;
using Microsoft.Win32;
[assembly: AssemblyTitle("@NAME@")]
[assembly: AssemblyProduct("@NAME@")]
[assembly: AssemblyCompany("OnGROW MSI test fixture")]
[assembly: AssemblyVersion("@VERSION@")]
[assembly: AssemblyFileVersion("@VERSION@")]
public sealed class OnGrowMsiProbe : ServiceBase {
    public OnGrowMsiProbe() { ServiceName = "@NAME@"; CanStop = true; }
    protected override void OnStart(string[] args) {
        using (RegistryKey key = Registry.LocalMachine.CreateSubKey("Software\\OnGROW\\MSIProbe\\customer-desk")) {
            if (key == null) throw new InvalidOperationException("Probe runtime marker unavailable");
            key.SetValue("RunningVersion", Assembly.GetExecutingAssembly().GetName().Version.ToString(), RegistryValueKind.String);
        }
    }
    protected override void OnStop() { }
    public static int Main(string[] args) {
        if (args.Length == 1 && args[0] == "--fail-update") return 17;
        if (args.Length == 1 && args[0] == "--service") {
            ServiceBase.Run(new OnGrowMsiProbe());
            return 0;
        }
        // Explicit strings are also checked before packaging the probe PE.
        Console.WriteLine("@NAME@\0ONGROW_MSI_PROBE_NO_NETWORK\0");
        Console.WriteLine("@VERSION@");
        return 0;
    }
}
