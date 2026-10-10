Feature: Places for planning a shopping trip
  Places are grouped (Grocery Store > LIDL, Aldi). A task can belong to several
  places. Each place shows how much there is to get there, so a trip can be
  planned. Adding a task from inside a place ticks that place for you.

  Scenario: Patrick sets up shops, Mona adds from inside LIDL, everyone sees the counts
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick creates the group "Grocery Store"
    And Patrick adds the place "LIDL" to "Grocery Store" at "LIDL Winnenden" and picks the suggestion "Marbacher"
    And Patrick adds the place "Aldi" to "Grocery Store" at "Aldi Waiblingen"
    Then Patrick sees the place "LIDL" at "71364 Winnenden"
    And Patrick sees the place "Aldi" at "Aldi Waiblingen"
    When Patrick adds the task "Milk" at "LIDL" and "Aldi"
    And Patrick adds the task "Bread" at "LIDL"
    Then Mona sees "LIDL" with 2 to get
    And Mona sees "Aldi" with 1 to get
    When Mona opens the place "LIDL"
    Then Mona sees "LIDL" ticked in the add bar
    When Mona adds the task "Cheese" here
    Then Mona sees the task "Cheese" in this place
    And Patrick sees "LIDL" with 3 to get
    And Mara sees "Aldi" with 1 to get
