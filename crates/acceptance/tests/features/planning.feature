Feature: Planning the next day
  At my planning time Upcoming opens and covers everything. On top the focus
  (three free slots), below my problems and what is left over. Saved, the plan
  is the next day's Today: my priorities, in my order.

  Scenario: Patrick plans tomorrow and gets it all done
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the tasks "Call the dentist", "Pay the bill" and "Book the holiday"
    And Mona has added the task "Water the plants"
    And Patrick shows All
    When Patrick gives "Call the dentist" to Patrick
    And Patrick gives "Pay the bill" to Patrick
    And Patrick gives "Book the holiday" to Patrick
    And Patrick notices "Out of food"
    And it is 20:00 for Patrick
    And Patrick sets his planning time to "19:00"
    Then Upcoming opens for Patrick
    And Upcoming of Patrick offers "Out of food"
    And Upcoming of Patrick offers "Call the dentist"
    When Patrick moves "Call the dentist" into the focus
    And Patrick steps into the problem "Out of food"
    And Patrick moves "Cook something" into the focus
    And Patrick goes back to all problems
    And Patrick drags "Book the holiday" to the top of the focus
    Then the focus of Patrick is "Book the holiday", "Call the dentist" and "Cook something"
    When Patrick swipes "Pay the bill" away
    Then Upcoming of Patrick does not offer "Pay the bill"
    When Patrick undoes "Pay the bill: not this time"
    Then Upcoming of Patrick offers "Pay the bill"
    When Patrick saves the plan
    Then Upcoming is closed for Patrick
    When the next morning comes for Patrick
    Then Patrick's priorities are "Book the holiday", "Call the dentist" and "Cook something"
    When Patrick drags "Cook something" to the top of his priorities
    Then Patrick's priorities are "Cook something", "Book the holiday" and "Call the dentist"
    When Patrick ticks "Book the holiday" off
    Then Patrick sees the text "1 done today"
    When Patrick ticks "Call the dentist" off
    And Patrick ticks "Cook something" off
    Then a kitten celebrates with Patrick
    When Patrick picks more
    And Patrick takes "Water the plants"
    Then Patrick's priorities are "Water the plants"

  Scenario: Not now: Upcoming waits half an hour
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When it is 21:00 for Patrick
    And Patrick sets his planning time to "20:30"
    Then Upcoming opens for Patrick
    When Patrick plans later
    Then Upcoming is closed for Patrick
    When it is 21:31 for Patrick
    Then Upcoming opens for Patrick
