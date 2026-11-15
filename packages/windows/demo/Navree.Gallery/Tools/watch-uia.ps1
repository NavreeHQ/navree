# Listens, as a UI Automation client in a process of its own, to the events a window raises, and writes each to a file
# until a stop file appears. The gallery starts it for the scenario that checks what assistive technology is told.
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$StopFile
)

Add-Type -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase -TypeDefinition @"
using System;
using System.IO;
using System.Threading;
using System.Windows.Automation;

public static class UiaWatch
{
    private static string log;

    private static void Say(string line)
    {
        lock (typeof(UiaWatch))
        {
            File.AppendAllText(log, DateTime.Now.ToString("HH:mm:ss.fff") + " " + line + "\r\n");
        }
    }

    private static string Who(object source)
    {
        try
        {
            AutomationElement element = (AutomationElement)source;
            return element.Current.ControlType.ProgrammaticName + " '" + element.Current.Name + "'";
        }
        catch (Exception)
        {
            return "(gone)";
        }
    }

    public static void Run(int processId, string path, string stop)
    {
        log = path;
        AutomationElement window = null;
        for (int i = 0; i < 50 && window == null; i++)
        {
            window = AutomationElement.RootElement.FindFirst(
                TreeScope.Children,
                new PropertyCondition(AutomationElement.ProcessIdProperty, processId));
            if (window == null)
            {
                Thread.Sleep(200);
            }
        }

        if (window == null)
        {
            Say("no window of the process was found");
            return;
        }

        Say("listening to " + Who(window));
        AutomationEventHandler told = delegate(object source, AutomationEventArgs e)
        {
            Say("event " + e.EventId.ProgrammaticName + " on " + Who(source));
        };
        Automation.AddAutomationEventHandler(SelectionItemPattern.ElementSelectedEvent, window, TreeScope.Subtree, told);
        Automation.AddAutomationEventHandler(SelectionItemPattern.ElementAddedToSelectionEvent, window, TreeScope.Subtree, told);
        Automation.AddAutomationEventHandler(SelectionItemPattern.ElementRemovedFromSelectionEvent, window, TreeScope.Subtree, told);
        Automation.AddAutomationEventHandler(SelectionPattern.InvalidatedEvent, window, TreeScope.Subtree, told);
        Automation.AddStructureChangedEventHandler(window, TreeScope.Subtree, delegate(object source, StructureChangedEventArgs e)
        {
            Say("structure " + e.StructureChangeType + " on " + Who(source));
        });
        Automation.AddAutomationPropertyChangedEventHandler(
            window,
            TreeScope.Subtree,
            delegate(object source, AutomationPropertyChangedEventArgs e)
            {
                Say("property " + e.Property.ProgrammaticName + " " + e.OldValue + " to " + e.NewValue + " on " + Who(source));
            },
            ExpandCollapsePattern.ExpandCollapseStateProperty,
            GridPattern.RowCountProperty,
            SelectionItemPattern.IsSelectedProperty);
        Say("handlers added");
        while (!File.Exists(stop))
        {
            Thread.Sleep(200);
        }

        Automation.RemoveAllEventHandlers();
        Say("stopped");
    }
}
"@

[UiaWatch]::Run($ProcessId, $Out, $StopFile)
