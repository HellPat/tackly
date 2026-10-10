Feature: Lists
  Tasks live in lists you step into; tasks without a list are in "Other",
  which is always open on the Tasks screen. Mine / Unassigned / All filter
  everywhere and keep their setting.

  Scenario: Patrick makes a list, fills it, renames it, and deletes it once it is done
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick creates the list "Garden"
    And Patrick opens "Garden"
    And Patrick adds the task "Mow the lawn"
    And Patrick shows All
    Then Patrick sees 1 in Unassigned
    And Mona sees the list "Garden"
    When Patrick renames the list to "Backyard"
    Then Mona sees the list "Backyard"
    When Patrick ticks "Mow the lawn" off
    And Patrick deletes the list
    Then Patrick and Mona do not see the list "Backyard"

  Scenario: The filter stays where it was put
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths"
    And Patrick has added the tasks "Call the dentist" and "Book the holiday"
    When Patrick shows All
    And Patrick gives "Call the dentist" to Patrick
    And Patrick shows Unassigned
    Then Patrick sees the task "Book the holiday"
    And Patrick does not see "Call the dentist"
    When Patrick creates the list "Garden"
    And Patrick opens "Garden"
    And Patrick goes back
    Then Patrick sees 1 in Unassigned
    And Patrick sees 1 in Mine
