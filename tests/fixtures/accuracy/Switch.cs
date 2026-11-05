class SwitchFixture {
    /// <summary>Exercise stacked labels and an independently observed default path.</summary>
    int Pick(int x) {
        switch (x) {
            case 1:
            case 2: Hits.Mark("Switch.cs", 6); return 1;
            default: Hits.Mark("Switch.cs", 7); return 0;
        }
    }
}
