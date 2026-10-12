Feature: Lists
  Tasks live in lists you step into; tasks without a list stand on their own
  on the Tasks screen. Mine / Unassigned / All filter everywhere and keep
  their setting. My Tasks is the Tasks screen filtered to Mine.

  Scenario: Tasks without a list stand on their own, and a task added on My Tasks is mine
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick opens the tab "Tasks"
    And Patrick adds the task "Water the plants"
    Then Patrick does not see the text "Other"
    And Patrick sees the task "Water the plants"
    When Patrick opens the tab "My Tasks"
    Then Patrick does not see "Water the plants"
    When Patrick adds the task "Pick up the parcel"
    Then Patrick sees the task "Pick up the parcel"
    And Patrick sees 1 in Mine
    When Mona opens the tab "Tasks"
    And Mona shows All
    Then Mona sees the task "Pick up the parcel"
    And Mona sees "Pick up the parcel" with "Patrick picked it"

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
