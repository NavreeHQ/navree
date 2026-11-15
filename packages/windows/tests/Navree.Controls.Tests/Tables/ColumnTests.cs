using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class ColumnSizingTests
{
    [TestMethod]
    [DataRow(ColumnKind.Title, 150.2, false, 0.0, 240.0)]
    [DataRow(ColumnKind.Title, 300.0, false, 0.0, 324.0)]
    [DataRow(ColumnKind.Words, 40.0, false, 0.0, 80.0)]
    [DataRow(ColumnKind.Words, 300.0, false, 0.0, 260.0)]
    [DataRow(ColumnKind.Number, 60.1, false, 0.0, 85.0)]
    [DataRow(ColumnKind.Number, 10.0, true, 0.0, 64.0)]
    [DataRow(ColumnKind.Number, 200.0, false, 0.0, 160.0)]
    [DataRow(ColumnKind.Status, 50.0, false, 0.0, 90.0)]
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
    public void A_table_a_little_too_wide_takes_the_room_from_the_name_before_it_scrolls()
    {
        // 752 wide in 704: the name gives 48 and nothing is cut off at the frozen end.
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 704);
        Same.Number(704, layout.RowWidth);
        Same.Number(192, layout.Columns[0].Width);
        Same.Number(232, layout.Fs);
        Same.Number(72, layout.Fe);
        Same.Number(400, layout.ScrollWidth);
        Same.Items([40.0, 232.0, 392.0, 512.0, 632.0], layout.Columns.Select(c => c.X));
    }

    [TestMethod]
    public void A_table_too_wide_even_then_scrolls_and_keeps_its_end_group()
    {
        // At 560 the name is not frozen (it would fill more than half), gives all it may (240 to 120) and 72 is still over.
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 560);
        Assert.AreEqual(0, layout.FrozenStartApplied);
        Same.Number(120, layout.Columns[0].Width);
        Same.Number(632, layout.RowWidth);
        Same.Number(40, layout.Fs);
        Same.Number(72, layout.Fe);
        Same.Number(520, layout.ScrollWidth);
        Assert.IsTrue(layout.EndFrozen);
        Same.Items([40.0, 160.0, 320.0, 440.0, 560.0], layout.Columns.Select(c => c.X));
        Same.Items(
            [ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.Scroll, ColumnGroup.FrozenEnd],
            layout.Columns.Select(c => c.Group));
    }

    [TestMethod]
    public void A_width_a_person_gave_to_the_name_is_not_taken_back()
    {
        ColumnSpec[] given =
        [
            new("title", ColumnKind.Title, 240, false),
            new("city", ColumnKind.Words, 160, false),
            new("amount", ColumnKind.Number, 120, false),
            new("stage", ColumnKind.Status, 120, false),
        ];
        ColumnLayout layout = ColumnLayoutBuilder.Build(given, 1, Actions, 40, 704);
        Same.Number(240, layout.Columns[0].Width);
        Same.Number(752, layout.RowWidth);
    }

    [TestMethod]
    public void A_viewport_a_hair_narrower_unfreezes_the_name()
    {
        ColumnLayout layout = ColumnLayoutBuilder.Build(Four(), 1, Actions, 40, 703);
        Assert.AreEqual(0, layout.FrozenStartApplied);
        Same.Number(40, layout.Fs);
        Same.Number(191, layout.Columns[0].Width);
        Same.Number(591, layout.ScrollWidth);
        Same.Number(703, layout.RowWidth);
        Same.Items([40.0, 231.0, 391.0, 511.0, 631.0], layout.Columns.Select(c => c.X));
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
        Same.Number(196, layout.Columns[0].Width);
        // Nothing is left to scroll: what the end group covers is nothing.
        Same.Number(958 - layout.Fe, layout.Columns[6].X + layout.Columns[6].Width);
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
