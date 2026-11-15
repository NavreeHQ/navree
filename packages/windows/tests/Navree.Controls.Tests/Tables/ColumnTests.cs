using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class ColumnSizingTests
{
    [TestMethod]
    [DataRow(ColumnKind.Title, 150.2, false, 0.0, 175.0)]
    [DataRow(ColumnKind.Title, 60.0, false, 0.0, 120.0)]
    [DataRow(ColumnKind.Title, 700.0, false, 0.0, 240.0)]
    [DataRow(ColumnKind.Words, 40.0, false, 0.0, 64.0)]
    [DataRow(ColumnKind.Words, 300.0, false, 0.0, 260.0)]
    [DataRow(ColumnKind.Number, 60.1, false, 0.0, 85.0)]
    [DataRow(ColumnKind.Number, 10.0, true, 0.0, 56.0)]
    [DataRow(ColumnKind.Number, 200.0, false, 0.0, 160.0)]
    [DataRow(ColumnKind.Status, 50.0, false, 0.0, 74.0)]
    [DataRow(ColumnKind.Status, 120.0, false, 0.0, 144.0)]
    [DataRow(ColumnKind.Status, 300.0, false, 0.0, 200.0)]
    [DataRow(ColumnKind.Controls, 0.0, false, 88.0, 112.0)]
    [DataRow(ColumnKind.Leading, 0.0, false, 0.0, 56.0)]
    public void A_column_asks_for_the_width_of_what_it_holds(ColumnKind kind, double widest, bool sortable, double control, double expected) =>
        Same.Number(expected, ColumnSizing.AutoWidth(kind, widest, sortable, control));

    [TestMethod]
    public void A_negative_text_width_is_refused() =>
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => ColumnSizing.AutoWidth(ColumnKind.Words, -1, false));

    [TestMethod]
    public void A_width_stays_in_its_kinds_range()
    {
        Same.Number(64, ColumnSizing.Clamp(ColumnKind.Words, 50));
        Same.Number(640, ColumnSizing.Clamp(ColumnKind.Words, 700));
        Same.Number(90, ColumnSizing.Clamp(ColumnKind.Controls, 90));
    }

    [TestMethod]
    public void A_key_changes_a_width_by_a_step()
    {
        Same.Number(208, ColumnSizing.ResizeByKey(ColumnKind.Words, 200, 1, false));
        Same.Number(232, ColumnSizing.ResizeByKey(ColumnKind.Words, 200, 1, true));
        Same.Number(64, ColumnSizing.ResizeByKey(ColumnKind.Words, 70, -1, true));
        Same.Number(640, ColumnSizing.ResizeByKey(ColumnKind.Words, 640, 1, false));
        Same.Number(90, ColumnSizing.ResizeByKey(ColumnKind.Controls, 90, 1, true));
    }

    [TestMethod]
    public void The_least_widths_are_the_kinds()
    {
        Same.Number(120, ColumnSizing.LeastWidth(ColumnKind.Title));
        Same.Number(64, ColumnSizing.LeastWidth(ColumnKind.Words));
        Same.Number(56, ColumnSizing.LeastWidth(ColumnKind.Number));
        Same.Number(72, ColumnSizing.LeastWidth(ColumnKind.Status));
        Same.Number(112, ColumnSizing.LeastWidth(ColumnKind.Controls, 88));
        Same.Number(56, ColumnSizing.LeastWidth(ColumnKind.Leading));
    }
}

[TestClass]
public sealed class ColumnLayoutTests
{
    [TestMethod]
    [DataRow(1200.0, 80.0, 72.0, 1, true)]
    [DataRow(700.0, 80.0, 72.0, 0, true)]
    [DataRow(704.0, 40.0, 72.0, 1, true)]
    [DataRow(703.99, 40.0, 72.0, 0, true)]
    [DataRow(160.0, 80.0, 72.0, 0, false)]
    public void Frozen_columns_get_half_the_viewport_at_most(double viewport, double gutter, double endWidth, int start, bool end)
    {
        (int s, bool e) = FrozenRegions.Allocate(viewport, gutter, endWidth, [240]);
        Assert.AreEqual(start, s);
        Assert.AreEqual(end, e);
    }

    [TestMethod]
    public void With_no_gutter_and_no_end_columns_the_first_column_freezes()
    {
        Assert.AreEqual((1, false), FrozenRegions.Allocate(641, 0, 0, [240]));
        Assert.AreEqual((2, true), FrozenRegions.Allocate(1000, 40, 72, [100, 150]));
        Assert.AreEqual((0, false), FrozenRegions.Allocate(100, 80, 0, [50]));
    }

    private static ColumnSpec[] Four() =>
    [
        new("title", ColumnKind.Title, 240, true),
        new("city", ColumnKind.Words, 160, false),
        new("amount", ColumnKind.Number, 120, false),
        new("stage", ColumnKind.Status, 120, false),
    ];

    private static readonly ColumnSpec[] Actions = [new("actions", ColumnKind.Controls, 56, false)];

    [TestMethod]
    public void Room_left_over_goes_to_the_flexible_column()
    {
        ColumnSpec[] three =
        [
            new("title", ColumnKind.Title, 240, true),
            new("city", ColumnKind.Words, 120, false),
            new("amount", ColumnKind.Number, 100, false),
        ];
        ColumnLayout layout = ColumnLayoutBuilder.Build(three, 1, Actions, 40, 1000);
        Same.Number(1000, layout.RowWidth);
        Same.Number(708, layout.Fs);
        Same.Number(72, layout.Fe);
        Same.Number(220, layout.ScrollWidth);
        Assert.AreEqual(1, layout.FrozenStartApplied);
        Same.Number(668, layout.Columns[0].Width);
        Same.Number(40, layout.Columns[0].X);
        Same.Number(708, layout.Columns[1].X);
        Same.Number(828, layout.Columns[2].X);
        Same.Number(928, layout.Columns[3].X);
        Assert.AreEqual(ColumnGroup.FrozenEnd, layout.Columns[3].Group);
    }

    [TestMethod]
    public void A_table_a_little_too_wide_takes_the_room_from_the_other_columns_and_keeps_the_name_whole()
    {
        // 752 wide in 704: the name has its 240 and what is over, 48, is taken from city, amount and stage in proportion to what each can give.
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 704);
        Same.Number(704, layout.RowWidth);
        Same.Number(240, layout.Columns[0].Width);
        Same.Items([137.0, 106.0, 109.0], layout.Columns.Skip(1).Take(3).Select(c => c.Width));
        Same.Number(280, layout.Fs);
        Same.Number(72, layout.Fe);
        Same.Number(352, layout.ScrollWidth);
        Same.Items([40.0, 280.0, 417.0, 523.0, 632.0], layout.Columns.Select(c => c.X));
    }

    [TestMethod]
    public void A_table_too_wide_even_after_the_other_columns_gave_takes_from_the_name_and_then_scrolls()
    {
        // At 400 the other columns give all they may (to 64, 56 and 72), the name gives 120 (240 to 120) and 24 is still over.
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 400);
        Assert.AreEqual(0, layout.FrozenStartApplied);
        Same.Items([120.0, 64.0, 56.0, 72.0], layout.Columns.Take(4).Select(c => c.Width));
        Same.Number(424, layout.RowWidth);
        Same.Number(40, layout.Fs);
        Same.Number(72, layout.Fe);
        Same.Number(312, layout.ScrollWidth);
        Assert.IsTrue(layout.EndFrozen);
        Same.Items([40.0, 160.0, 224.0, 280.0, 352.0], layout.Columns.Select(c => c.X));
        Same.Items(
            [ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.FrozenEnd],
            layout.Columns.Select(c => c.Group));
    }

    [TestMethod]
    public void A_name_asking_for_more_than_its_words_need_gives_the_rest_before_any_other_column_does()
    {
        ColumnSpec[] wide =
        [
            new("title", ColumnKind.Title, 400, true),
            new("city", ColumnKind.Words, 160, false),
            new("amount", ColumnKind.Number, 120, false),
        ];
        // 680 wide in 600: the name has 160 above its need and gives 80 of it; city and amount keep what they asked for.
        ColumnLayout layout = ColumnLayoutBuilder.Build(wide, 0, [], 0, 600);
        Same.Items([320.0, 160.0, 120.0], layout.Columns.Select(c => c.Width));
        Same.Number(600, layout.RowWidth);
    }

    [TestMethod]
    public void A_width_a_person_gave_to_a_column_is_not_taken_back_to_make_room()
    {
        ColumnSpec[] given =
        [
            new("title", ColumnKind.Title, 240, true),
            new("city", ColumnKind.Words, 160, false, Sized: true),
            new("amount", ColumnKind.Number, 120, false),
            new("stage", ColumnKind.Status, 120, false),
        ];
        ColumnLayout layout = ColumnLayoutBuilder.Build(given, 0, [], 0, 540);
        Same.Number(160, layout.Columns[1].Width);
        Same.Number(240, layout.Columns[0].Width);
        Same.Number(540, layout.RowWidth);
        Same.Items([62.0, 78.0], layout.Columns.Skip(2).Select(c => c.Width));
    }

    [TestMethod]
    public void The_inventory_of_the_store_demo_at_840_gives_the_name_its_words_and_the_other_columns_give_first()
    {
        // Product (the name), Category, In stock, Level, and the field with its button, as the table measures them; Price is secondary.
        // The window is 840 wide and the table 709: its scroll view is 707.
        ColInfo[] inventory =
        [
            new("product", ColumnKind.Title, ColumnRole.Title, false),
            new("category", ColumnKind.Words, ColumnRole.None, false),
            new("stock", ColumnKind.Number, ColumnRole.None, false),
            new("level", ColumnKind.Status, ColumnRole.Status, false),
            new("restock", ColumnKind.Controls, ColumnRole.None, false),
            new("price", ColumnKind.Number, ColumnRole.None, true),
        ];
        double[] asks = [240, 112, 104, 112, 224, 96];
        IReadOnlyList<int> shown = ColumnSet.Shown(inventory, new HashSet<string>(), new HashSet<string>(), asks, 707);
        Same.Items([0, 1, 2, 3, 4], shown);

        ColumnSpec[] specs = [.. shown.Select(i => new ColumnSpec(inventory[i].Key, inventory[i].Kind, asks[i], inventory[i].Kind == ColumnKind.Title))];
        ColumnLayout layout = ColumnLayoutBuilder.Build(specs, 1, [], 0, 707);
        Same.Number(240, layout.Columns[0].Width);
        Same.Items([82.0, 74.0, 87.0], layout.Columns.Skip(1).Take(3).Select(c => c.Width));
        Same.Number(224, layout.Columns[4].Width);
        Same.Number(707, layout.RowWidth);
        Assert.IsTrue(layout.Columns.Skip(1).Take(3).Select(c => c.Width).Zip([64.0, 56.0, 72.0]).All(p => p.First >= p.Second), "no column goes below its least");
    }

    [TestMethod]
    public void A_width_a_person_gave_to_the_name_is_not_taken_back_and_the_other_columns_give_instead()
    {
        ColumnSpec[] given =
        [
            new("title", ColumnKind.Title, 240, false, Sized: true),
            new("city", ColumnKind.Words, 160, false),
            new("amount", ColumnKind.Number, 120, false),
            new("stage", ColumnKind.Status, 120, false),
        ];
        ColumnLayout layout = ColumnLayoutBuilder.Build(given, 1, Actions, 40, 704);
        Same.Number(240, layout.Columns[0].Width);
        Same.Number(704, layout.RowWidth);
        Same.Items([137.0, 106.0, 109.0], layout.Columns.Skip(1).Take(3).Select(c => c.Width));
    }

    [TestMethod]
    public void A_viewport_a_hair_narrower_unfreezes_the_name()
    {
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 703);
        Assert.AreEqual(0, layout.FrozenStartApplied);
        Same.Number(40, layout.Fs);
        Same.Number(240, layout.Columns[0].Width);
        Same.Items([137.0, 105.0, 109.0], layout.Columns.Skip(1).Take(3).Select(c => c.Width));
        Same.Number(703, layout.RowWidth);
    }

    [TestMethod]
    public void The_columns_of_the_sample_table_each_get_the_room_their_words_need_and_no_cell_is_cut()
    {
        // The words each column holds, as the table measures them at 100 percent for the sample's rows: the longest name is "Manpreet Sandhu".
        (string Key, ColumnKind Kind, double Words, bool Sort)[] columns =
        [
            ("name", ColumnKind.Title, 105, true),
            ("city", ColumnKind.Words, 72, true),
            ("phone", ColumnKind.Words, 106, false),
            ("plan", ColumnKind.Words, 55, false),
            ("amount", ColumnKind.Number, 59, true),
            ("status", ColumnKind.Status, 50, false),
            ("seen", ColumnKind.Words, 107, false),
        ];
        double[] asks = [.. columns.Select(c => ColumnSizing.AutoWidth(c.Kind, c.Words, c.Sort))];
        ColInfo[] infos = [.. columns.Select(c => new ColInfo(c.Key, c.Kind, c.Kind == ColumnKind.Title ? ColumnRole.Title : ColumnRole.None, false))];
        IReadOnlyList<int> shown = ColumnSet.Shown(infos, new HashSet<string>(), new HashSet<string>(), asks, 958 - 80);
        Same.Items(Enumerable.Range(0, 7), shown);
        ColumnSpec[] specs = [.. columns.Select((c, i) => new ColumnSpec(c.Key, c.Kind, asks[i], c.Kind == ColumnKind.Title))];

        // At the width of the gallery window with the check box and the chevron before the name, and the actions kept at the end.
        ColumnLayout layout = ColumnLayoutBuilder.Build(specs, 1, Actions, 80, 958);
        for (int i = 1; i < 7; i++)
        {
            Assert.IsTrue(layout.Columns[i].Width >= asks[i], $"{columns[i].Key} has {layout.Columns[i].Width} and its words need {asks[i]}");
        }

        Assert.IsTrue(layout.Columns[0].Width >= asks[0], "the name has what its words need too");
        Same.Number(958, layout.RowWidth);
        Assert.AreEqual(layout.Columns[0].Width, 958 - 80 - 72 - asks.Skip(1).Sum(), 1e-6, "what is left over goes to the name");
    }

    [TestMethod]
    public void One_very_long_name_asks_for_no_more_than_a_name_needs_and_does_not_push_the_others_out()
    {
        double name = ColumnSizing.AutoWidth(ColumnKind.Title, 700, false);
        Same.Number(ColumnSet.TitleNeed, name);
        ColumnSpec[] specs =
        [
            new("name", ColumnKind.Title, name, true),
            new("city", ColumnKind.Words, ColumnSizing.AutoWidth(ColumnKind.Words, 92, false), false),
            new("phone", ColumnKind.Words, ColumnSizing.AutoWidth(ColumnKind.Words, 106, false), false),
            new("amount", ColumnKind.Number, ColumnSizing.AutoWidth(ColumnKind.Number, 79, false), false),
        ];
        double[] asks = [.. specs.Select(s => s.Width)];
        ColumnLayout layout = ColumnLayoutBuilder.Build(specs, 1, Actions, 40, 958);
        for (int i = 1; i < 4; i++)
        {
            Same.Number(asks[i], layout.Columns[i].Width);
        }

        Assert.IsTrue(layout.Columns[0].Width >= ColumnSet.TitleNeed);
        Same.Number(958, layout.RowWidth);
    }

    [TestMethod]
    public void A_column_the_person_fits_takes_all_its_words_whatever_it_asks_for_by_itself()
    {
        Same.Number(724, ColumnSizing.FitWidth(ColumnKind.Title, 700, false));
        Same.Number(800, ColumnSizing.FitWidth(ColumnKind.Title, 2000, true));
        Same.Number(120, ColumnSizing.FitWidth(ColumnKind.Title, 10, false));
        Same.Number(64, ColumnSizing.FitWidth(ColumnKind.Words, 10, false));
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => ColumnSizing.FitWidth(ColumnKind.Words, -1, false));
    }

    [TestMethod]
    public void The_columns_of_the_sample_table_fit_a_viewport_of_958_without_scrolling()
    {
        // Name, City, Phone, Plan, Amount, Status, Last seen and the actions, as measured at 100 percent: 1002 wide before.
        ColumnSpec[] shown =
        [
            new("name", ColumnKind.Title, 240, true),
            new("city", ColumnKind.Words, 116, false),
            new("phone", ColumnKind.Words, 130, false),
            new("plan", ColumnKind.Words, 80, false),
            new("amount", ColumnKind.Number, 103, false),
            new("status", ColumnKind.Status, 90, false),
            new("seen", ColumnKind.Words, 131, false),
        ];
        ColumnLayout layout = ColumnLayoutBuilder.Build(shown, 1, Actions, 40, 958);
        Same.Number(958, layout.RowWidth);
        Same.Number(240, layout.Columns[0].Width);
        // Nothing is left to scroll: what the end group covers is nothing.
        Same.Number(958 - layout.Fe, layout.Columns[6].X + layout.Columns[6].Width);
        Assert.IsTrue(layout.Columns.Skip(1).Take(6).All(c => c.Width >= 56), "no column goes below its least");
    }

    [TestMethod]
    public void A_table_with_no_columns_is_as_wide_as_the_viewport()
    {
        ColumnLayout layout = ColumnLayoutBuilder.Build([], 0, [], 0, 800);
        Same.Number(800, layout.RowWidth);
        Assert.AreEqual(0, layout.Columns.Count);
    }
}

[TestClass]
public sealed class StickyMathTests
{
    [TestMethod]
    public void A_scroll_range_follows_the_flow()
    {
        Assert.AreEqual((0.0, 400.0), StickyMath.PositionRange(1000, 600, false));
        Assert.AreEqual((-400.0, 0.0), StickyMath.PositionRange(1000, 600, true));
        Assert.AreEqual((0.0, 0.0), StickyMath.PositionRange(500, 600, true));
    }

    [TestMethod]
    [DataRow(0.0, 0.0, -400.0, 0.0, 280.0, 472.0, 0.0, 1.0)]
    [DataRow(250.0, 250.0, -150.0, -250.0, 530.0, 222.0, 1.0, 1.0)]
    [DataRow(400.0, 400.0, 0.0, -400.0, 680.0, 72.0, 1.0, 0.0)]
    [DataRow(4.0, 4.0, -396.0, -4.0, 284.0, 468.0, 0.5, 1.0)]
    [DataRow(396.0, 396.0, -4.0, -396.0, 676.0, 76.0, 1.0, 0.5)]
    public void Left_to_right_what_is_kept_follows_the_position(
        double position, double start, double end, double header, double left, double right, double edgeStart, double edgeEnd)
    {
        Same.Number(start, StickyMath.StartTranslation(position, false));
        Same.Number(end, StickyMath.EndTranslation(position, 0, 400, false));
        Same.Number(header, StickyMath.HeaderTranslation(position, false));
        (double l, double r) = StickyMath.ScrollClipInsets(position, 0, 400, 280, 72, false);
        Same.Number(left, l);
        Same.Number(right, r);
        Same.Number(edgeStart, StickyMath.EdgeOpacityStart(position, 0, 400, false));
        Same.Number(edgeEnd, StickyMath.EdgeOpacityEnd(position, 0, 400, false));
    }

    // The scroll view's positions are negative in right to left, but a translation and a clip are counted
    // from the start in both flows (as the scroll view reports them), so the same rows scrolled by the same amount give the same numbers.
    [TestMethod]
    [DataRow(0.0, 0.0, -400.0, 0.0, 280.0, 472.0, 0.0, 1.0)]
    [DataRow(-250.0, 250.0, -150.0, -250.0, 530.0, 222.0, 1.0, 1.0)]
    [DataRow(-400.0, 400.0, 0.0, -400.0, 680.0, 72.0, 1.0, 0.0)]
    [DataRow(-4.0, 4.0, -396.0, -4.0, 284.0, 468.0, 0.5, 1.0)]
    [DataRow(-396.0, 396.0, -4.0, -396.0, 676.0, 76.0, 1.0, 0.5)]
    public void Right_to_left_the_same_numbers_hold_from_the_negative_positions_the_scroll_view_gives(
        double position, double start, double end, double header, double left, double right, double edgeStart, double edgeEnd)
    {
        Same.Number(start, StickyMath.StartTranslation(position, true));
        Same.Number(end, StickyMath.EndTranslation(position, -400, 0, true));
        Same.Number(header, StickyMath.HeaderTranslation(position, true));
        (double l, double r) = StickyMath.ScrollClipInsets(position, -400, 0, 280, 72, true);
        Same.Number(left, l);
        Same.Number(right, r);
        Same.Number(edgeStart, StickyMath.EdgeOpacityStart(position, -400, 0, true));
        Same.Number(edgeEnd, StickyMath.EdgeOpacityEnd(position, -400, 0, true));
    }

    [TestMethod]
    public void The_expressions_the_compositor_runs_give_the_same_numbers_as_the_functions()
    {
        foreach (bool rtl in new[] { false, true })
        {
            (double min, double max) = StickyMath.PositionRange(1000, 600, rtl);
            foreach (double fraction in new[] { 0.0, 0.01, 0.37, 0.5, 0.99, 1.0 })
            {
                double position = min + ((max - min) * fraction);
                Dictionary<string, double> names = new()
                {
                    ["scroll.Position.X"] = position,
                    ["scroll.MinPosition.X"] = min,
                    ["scroll.MaxPosition.X"] = max,
                    ["layout.Fs"] = 280,
                    ["layout.Fe"] = 72,
                };

                Same.Number(StickyMath.StartTranslation(position, rtl), Evaluate(StickyMath.StartExpression(rtl), names));
                Same.Number(StickyMath.EndTranslation(position, min, max, rtl), Evaluate(StickyMath.EndExpression(rtl), names));
                Same.Number(StickyMath.HeaderTranslation(position, rtl), Evaluate(StickyMath.HeaderExpression(rtl), names));
                (double left, double right) = StickyMath.ScrollClipInsets(position, min, max, 280, 72, rtl);
                (string leftExpression, string rightExpression) = StickyMath.ClipExpressions(rtl);
                Same.Number(left, Evaluate(leftExpression, names));
                Same.Number(right, Evaluate(rightExpression, names));
                (string startEdge, string endEdge) = StickyMath.EdgeExpressions(rtl);
                Same.Number(StickyMath.EdgeOpacityStart(position, min, max, rtl), Evaluate(startEdge, names));
                Same.Number(StickyMath.EdgeOpacityEnd(position, min, max, rtl), Evaluate(endEdge, names));
            }
        }
    }

    [TestMethod]
    public void The_two_flows_scrolled_the_same_way_give_the_same_numbers()
    {
        foreach (double scrolled in new[] { 0.0, 3.0, 120.0, 400.0 })
        {
            Same.Number(StickyMath.StartTranslation(scrolled, false), StickyMath.StartTranslation(-scrolled, true));
            Same.Number(StickyMath.EndTranslation(scrolled, 0, 400, false), StickyMath.EndTranslation(-scrolled, -400, 0, true));
            Same.Number(StickyMath.HeaderTranslation(scrolled, false), StickyMath.HeaderTranslation(-scrolled, true));
            Assert.AreEqual(StickyMath.ScrollClipInsets(scrolled, 0, 400, 280, 72, false), StickyMath.ScrollClipInsets(-scrolled, -400, 0, 280, 72, true));
        }
    }

    // A small evaluator for the expressions: numbers, names with dots, + - * /, brackets, a minus sign, and Clamp.
    private static double Evaluate(string expression, Dictionary<string, double> names)
    {
        Parser parser = new(expression, names);
        double value = parser.Sum();
        Assert.IsTrue(parser.AtEnd, "the expression was not read to its end: " + expression);
        return value;
    }

    private sealed class Parser(string text, Dictionary<string, double> names)
    {
        private int at;

        internal bool AtEnd
        {
            get
            {
                Skip();
                return at >= text.Length;
            }
        }

        internal double Sum()
        {
            double value = Product();
            while (true)
            {
                Skip();
                if (at < text.Length && text[at] == '+')
                {
                    at++;
                    value += Product();
                }
                else if (at < text.Length && text[at] == '-')
                {
                    at++;
                    value -= Product();
                }
                else
                {
                    return value;
                }
            }
        }

        private double Product()
        {
            double value = Unary();
            while (true)
            {
                Skip();
                if (at < text.Length && text[at] == '*')
                {
                    at++;
                    value *= Unary();
                }
                else if (at < text.Length && text[at] == '/')
                {
                    at++;
                    value /= Unary();
                }
                else
                {
                    return value;
                }
            }
        }

        private double Unary()
        {
            Skip();
            if (at < text.Length && text[at] == '-')
            {
                at++;
                return -Unary();
            }

            if (at < text.Length && text[at] == '(')
            {
                at++;
                double inner = Sum();
                Expect(')');
                return inner;
            }

            if (at < text.Length && (char.IsDigit(text[at]) || text[at] == '.'))
            {
                int start = at;
                while (at < text.Length && (char.IsDigit(text[at]) || text[at] == '.'))
                {
                    at++;
                }

                return double.Parse(text[start..at], System.Globalization.CultureInfo.InvariantCulture);
            }

            int nameStart = at;
            while (at < text.Length && (char.IsLetterOrDigit(text[at]) || text[at] == '.' || text[at] == '_'))
            {
                at++;
            }

            string name = text[nameStart..at];
            Skip();
            if (name == "Clamp" && at < text.Length && text[at] == '(')
            {
                at++;
                double value = Sum();
                Expect(',');
                double low = Sum();
                Expect(',');
                double high = Sum();
                Expect(')');
                return Math.Clamp(value, low, high);
            }

            Assert.IsTrue(names.ContainsKey(name), "unknown name in the expression: '" + name + "' in " + text);
            return names[name];
        }

        private void Expect(char c)
        {
            Skip();
            Assert.IsTrue(at < text.Length && text[at] == c, "expected '" + c + "' at " + at + " in " + text);
            at++;
        }

        private void Skip()
        {
            while (at < text.Length && text[at] == ' ')
            {
                at++;
            }
        }
    }
}
