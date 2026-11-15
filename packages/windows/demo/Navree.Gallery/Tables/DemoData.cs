using System.Globalization;
using Navree.Controls;
using Navree.Design.Tables;

namespace Navree.Gallery;

/// <summary>A customer of the demo: made-up people in North Indian cities, with amounts in rupees.</summary>
/// <param name="Id">The customer's key.</param>
/// <param name="Name">The name.</param>
/// <param name="City">The city.</param>
/// <param name="Phone">The telephone number.</param>
/// <param name="Email">The mail address.</param>
/// <param name="Plan">The plan.</param>
/// <param name="Amount">What the customer owes or paid, in rupees.</param>
/// <param name="Status">The state of the account.</param>
/// <param name="LastSeen">When the customer was last seen, in Indian time.</param>
internal sealed record Customer(string Id, string Name, string City, string Phone, string Email, string Plan, long Amount, string Status, string LastSeen);

/// <summary>Makes rows for the tables of the gallery.</summary>
internal static class DemoData
{
    private static readonly string[] First =
    [
        "Harpreet", "Gurpreet", "Rajesh", "Simran", "Amit", "Neha", "Karan", "Pooja", "Vikram", "Anjali",
        "Sandeep", "Ritu", "Manpreet", "Jaspreet", "Tarun", "Kavita", "Rohit", "Seema", "Deepak", "Meena",
        "Balwinder", "Lakshmi", "Yash", "Preeti", "Ajay", "Nisha", "Gaurav", "Sunita", "Mohit", "Komal",
    ];

    private static readonly string[] Last =
    [
        "Kaur", "Singh", "Sharma", "Verma", "Gupta", "Malhotra", "Arora", "Chauhan", "Bhatia", "Kumar",
        "Sethi", "Gill", "Dhillon", "Khanna", "Kapoor", "Mehra", "Walia", "Grewal", "Sandhu", "Bansal",
    ];

    private static readonly string[] Cities =
    [
        "Ludhiana", "Amritsar", "Jalandhar", "Patiala", "Chandigarh", "Delhi", "Jaipur", "Lucknow", "Dehradun", "Shimla",
    ];

    private static readonly string[] Plans = ["Free", "Pro", "Business"];

    private static readonly string[] Statuses = ["Active", "Paused", "Overdue", "Active", "Active"];

    private static readonly CultureInfo India = CultureInfo.GetCultureInfo("en-IN");

    /// <summary>The culture the demo writes numbers in.</summary>
    internal static CultureInfo Culture => India;

    /// <summary>Makes customers, the same ones for the same numbers.</summary>
    /// <param name="first">The number of the first.</param>
    /// <param name="count">How many.</param>
    /// <returns>The customers.</returns>
    internal static List<Customer> Make(int first, int count) =>
        Enumerable.Range(first, count).Select(Make).ToList();

    /// <summary>Makes one customer.</summary>
    /// <param name="i">Its number.</param>
    /// <returns>The customer.</returns>
    internal static Customer Make(int i)
    {
        string given = First[(i * 7) % First.Length];
        string family = Last[(i * 3 + (i / 30)) % Last.Length];
        string name = given + " " + family;
        string email = (given + "." + family).ToLowerInvariant() + "@example.in";
        string phone = string.Create(CultureInfo.InvariantCulture, $"+91 {98000 + ((i * 37) % 1000):00000} {10000 + ((i * 7919) % 90000):00000}");
        long amount = 1000 + ((i * 7919L) % 500_000);
        int day = 1 + ((i * 5) % 28);
        string seen = string.Create(CultureInfo.InvariantCulture, $"{day:00} Nov, {(i * 3) % 24:00}:{(i * 11) % 60:00} IST");
        return new Customer(
            "c" + i.ToString("00000", CultureInfo.InvariantCulture),
            name,
            Cities[(i * 11) % Cities.Length],
            phone,
            email,
            Plans[i % Plans.Length],
            amount,
            Statuses[(i * 13) % Statuses.Length],
            seen);
    }

    /// <summary>The rupees of an amount, grouped as the lakh is: 1,00,000.</summary>
    /// <param name="amount">The amount.</param>
    /// <returns>The words.</returns>
    internal static string Rupees(long amount) => "₹" + amount.ToString("N0", India);

    /// <summary>The columns of the customers table.</summary>
    /// <param name="sorted">The key of the column that is sorted, or null.</param>
    /// <param name="descending">Whether the sort is descending.</param>
    /// <returns>The model.</returns>
    internal static ListDataTableModel<Customer> Model(string? sorted = null, bool descending = false)
    {
        ListDataTableModel<Customer> model = new(
            [
                new DataTableColumn<Customer>("name", "Name", c => c.Name, ColumnKind.Title, Sortable: true, Role: ColumnRole.Title),
                new DataTableColumn<Customer>("city", "City", c => c.City, ColumnKind.Words, Sortable: true),
                new DataTableColumn<Customer>("phone", "Phone", c => c.Phone, ColumnKind.Words),
                new DataTableColumn<Customer>("email", "Email", c => c.Email, ColumnKind.Words, Secondary: true),
                new DataTableColumn<Customer>("plan", "Plan", c => c.Plan, ColumnKind.Words),
                new DataTableColumn<Customer>("amount", "Amount", c => Rupees(c.Amount), ColumnKind.Number, DataTableAlign.End, Sortable: true, Role: ColumnRole.Trailing),
                new DataTableColumn<Customer>("status", "Status", c => c.Status, ColumnKind.Status, Role: ColumnRole.Status),
                new DataTableColumn<Customer>("seen", "Last seen", c => c.LastSeen, ColumnKind.Words),
                new DataTableColumn<Customer>("actions", "Actions", _ => string.Empty, ColumnKind.Controls, Role: ColumnRole.Actions),
            ],
            c => c.Id)
        {
            HasMenuOf = _ => true,
            HasActionOf = _ => true,
        };
        model.Choice = new DataTableChoiceOptions(true, true, 1204);
        if (sorted is not null)
        {
            model.SetSort(sorted, descending ? DataTableSortDirection.Descending : DataTableSortDirection.Ascending);
        }

        return model;
    }
}
