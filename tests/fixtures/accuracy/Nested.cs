class NestedFixture {
    /// <summary>Keep the outer decision and local callable coverage independently observable.</summary>
    int Outer(int x) {
        /// <summary>Record execution owned by the local callable.</summary>
        int Inner(int y) => Hits.Value("Nested.cs", 5, y);
        if (x > 0) { Hits.Mark("Nested.cs", 6); return Inner(x); }
        Hits.Mark("Nested.cs", 7); return 0;
    }
}
